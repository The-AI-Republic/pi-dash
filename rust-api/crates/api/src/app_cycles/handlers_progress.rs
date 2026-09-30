//! Cycle progress handler (D-27, stage 5, PIDASHCONV-410).
//!
//! Ports `CycleProgressEndpoint.get`
//! (`apps/api/pi_dash/app/views/cycle/base.py:658-783`, drift baseline
//! `01a93e17`) with identical URL path, status codes and JSON bytes:
//!
//! - `GET workspaces/<slug>/projects/<id>/cycles/<uuid>/progress/`
//!   (`app/urls/cycle.py`, routes 13-14 family).
//!
//! Handler notes (all verified against the Python source):
//! - Gate is GUEST (`base.py:659`, [`crate::app_cycles::gates`] row
//!   `GET .../progress/`): Django-session authN first (anon 401), then
//!   the `@allow_permission` PROJECT check (403), then the body.
//! - `project_id` is a `<str:>` converter: a non-UUID value reaches the
//!   view, where the ORM UUID validation raises `ValidationError` and
//!   `handle_exception` answers 400
//!   `{"error":"Please provide valid detail"}` (`app/views/base.py`).
//! - `cycle_id` is a `<uuid:>` converter: a non-UUID tail never reaches
//!   the view (Django routing 404). Axum captures any segment, so the
//!   handler proxies those tails to Django instead of inventing a 400.
//! - The cycle lookup uses the plain `Cycle.objects` manager
//!   (`base.py:661`): no `deleted_at` predicate — a soft-deleted cycle
//!   still answers 200 rather than 404. Ported as-is.
//! - Estimate aggregates (`:664-711`) scope to issues whose
//!   `estimate_point.estimate.type` is `"points"` with a live bridge
//!   row, plus the `IssueManager` base excludes (not triage, not
//!   archived, live project, not draft). Per-group `Case`/`Sum`s with no
//!   default come back `NULL` on empty and render through `or 0`
//!   (`:769-773`): `None` AND `0.0` both answer int `0`; only nonzero
//!   sums render as floats. `total` uses `Sum(default=0)` (`:710`) and
//!   is NOT passed through `or 0` (`:774`): empty answers int `0`, a
//!   real zero sum answers `0.0`.
//! - Counts (`:712-765`): when `progress_snapshot` is truthy the six
//!   counts come from the snapshot keys with `.get(key, 0)` defaults;
//!   otherwise six live `COUNT`s run (same bridge + manager scope, no
//!   archived/draft guards — differs from the list counts, ported).
//! - Envelope key order is the Python literal order (`:767-783`),
//!   including the `total_issues` second quirk and the singular
//!   aggregate names mapped to plural keys.
//!
//! Fixture oracle: F-C27-08
//! (`rust-api/fixtures/app_cycles/progress.json` + `TRACE.md`); the unit
//! tests below pin the envelope order, the `or 0` rule and the snapshot
//! branch against that file so transcription drift fails the build.
//!
//! Ported bugs (translation, don't redesign; also listed in the PR):
//! - B1: no `deleted_at` guard on the cycle lookup (plain manager) —
//!   soft-deleted cycles answer 200 with live-computed data.
//! - B2: the estimate aggregate and the live counts carry no
//!   archived/is-draft guards beyond the manager excludes (differs from
//!   the list endpoint, which counts drafts/archived differently).
//! - B3: `0.0` estimate sums collapse to int `0` via `or 0`, while
//!   `total` keeps `0.0` — the two keys disagree on zero by design of
//!   the source, not by rounding.
//!
//! Sibling plumbing mirrors the D-29 `app_views_search` shape:
//! [`owned`] (unowned methods proxy), session [`actor`], exact denial
//! bodies, and manual envelope assembly for DRF key order/bytes.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::{Map, Value};

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::gates::{decide_gate, gate_for, tenant_context, GateOutcome, ANON_BODY, FORBIDDEN_BODY};

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Route path template for the progress endpoint, in `app/urls/cycle.py`
/// form (the `GET .../progress/` row also lives in
/// [`crate::app_cycles::gates::GATES`]).
pub const PROGRESS_PATH: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/progress/";

/// Register the progress GET route. Nothing else: sibling paths stay
/// unmatched and proxy to Django, and every non-owned method on the
/// owned path falls through to Django too (its 401-anon-before-405 and
/// DRF metadata live there).
pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/progress/",
        owned(axum::routing::get(progress_get), &["GET"]),
    )
}

/// An owned path: listed methods serve from Rust, everything else proxies
/// to Django. OPTIONS proxies too: DRF answers metadata (401 anon / 200
/// authed) where axum would 405. HEAD rides axum's `get` handling like
/// Django's `GET`-backed `HEAD`.
fn owned(
    router: axum::routing::MethodRouter<AppState>,
    methods: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = router;
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        if methods.contains(&method) {
            continue;
        }
        router = match method {
            "GET" => router.get(crate::edge::proxy),
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            _ => router.options(crate::edge::proxy),
        };
    }
    router
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// `Cycle not found` 404 (`base.py:661-663`).
pub const CYCLE_NOT_FOUND_BODY: &str = r#"{"error":"Cycle not found"}"#;
/// ORM `ValidationError` 400 (`app/views/base.py:126-130`): a non-UUID
/// `project_id` reaches the view through the `<str:>` converter.
pub const VALIDATION_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// `handle_exception`'s generic 500 branch.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// Handler denials with byte-exact bodies.
pub enum Denial {
    Unauthorized,
    Forbidden,
    NotFound,
    BadValidation,
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, ANON_BODY.to_owned()),
            Denial::Forbidden => (StatusCode::FORBIDDEN, FORBIDDEN_BODY.to_owned()),
            Denial::NotFound => (StatusCode::NOT_FOUND, CYCLE_NOT_FOUND_BODY.to_owned()),
            Denial::BadValidation => (StatusCode::BAD_REQUEST, VALIDATION_BODY.to_owned()),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = self.status_and_body();
        (
            status,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response()
    }
}

fn json_response(status: StatusCode, body: String) -> Response {
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Request context: auth + tenant + membership
// ---------------------------------------------------------------------------

/// Session auth (`BaseSessionAuthentication` + `IsAuthenticated`):
/// anonymous answers the DRF `NotAuthenticated` body before anything
/// else runs.
async fn actor(
    state: &AppState,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<crate::license::Actor, Denial> {
    let pool = pool_of(state)?;
    crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::Unauthorized)
}

fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// Membership facts for the GUEST progress gate, resolved with the same
/// row filters Python uses: active, non-deleted rows scoped to the
/// workspace slug (and project id for the project row).
async fn membership(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<(Option<i32>, Option<i32>), Denial> {
    let workspace_role: Option<(Option<i16>,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id AND w.deleted_at IS NULL
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let project_role: Option<(Option<i16>,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id AND w.deleted_at IS NULL
           WHERE w.slug = $1 AND pm.project_id = $2 AND pm.member_id = $3
             AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok((
        workspace_role.and_then(|row| row.0).map(i32::from),
        project_role.and_then(|row| row.0).map(i32::from),
    ))
}

// ---------------------------------------------------------------------------
// SQL: estimate aggregates + live counts
// ---------------------------------------------------------------------------

/// Shared issue scope for every progress query (`base.py:664-671` plus
/// the `IssueManager` base excludes): live bridge row, workspace slug
/// and project of the issue itself, `estimate_point` on a `"points"`
/// estimate, not triage / not archived / live project / not draft.
///
/// The triage exclusion is `NOT (group = 'triage')` over the join —
/// three-valued, so `NULL`-state rows drop with it (the app_issues
/// precedent). Joined tables carry no `deleted_at` predicate: Django
/// applies the soft-delete manager to the base table only.
const ISSUE_SCOPE_SQL: &str = "FROM issues i \
     JOIN cycle_issues ci ON ci.issue_id = i.id AND ci.cycle_id = $3 AND ci.deleted_at IS NULL \
     JOIN workspaces w ON w.id = i.workspace_id AND w.slug = $1 \
     JOIN projects p ON p.id = i.project_id AND p.id = $2 AND p.archived_at IS NULL \
     LEFT JOIN states s ON s.id = i.state_id \
     JOIN estimate_points ep ON ep.id = i.estimate_point_id \
     JOIN estimates e ON e.id = ep.estimate_id AND e.type = 'points' \
     WHERE i.deleted_at IS NULL AND i.archived_at IS NULL AND i.is_draft = FALSE \
     AND NOT (s.group = 'triage')";

/// One-row estimate aggregate (`base.py:664-711`): five per-group
/// `Case`/`Sum`s (NULL on empty — the caller applies `or 0`) plus the
/// `total` `Sum(default=0)` (never NULL — the caller renders it raw).
fn estimates_sql() -> String {
    let group = |name: &str| {
        format!(
            "SUM(CASE WHEN s.group = '{name}' THEN CAST(ep.value AS DOUBLE PRECISION) ELSE 0 END) AS {name}"
        )
    };
    format!(
        "SELECT {} {}",
        [
            group("backlog"),
            group("unstarted"),
            group("started"),
            group("cancelled"),
            group("completed"),
            "SUM(CAST(ep.value AS DOUBLE PRECISION)) AS total".to_owned(),
        ]
        .join(", "),
        ISSUE_SCOPE_SQL
    )
}

/// Six live counts plus the total in one round trip (`base.py:719-765`):
/// same bridge + slug + project + manager scope as the aggregate, one
/// `FILTER` count per state group. Result-identical to the six separate
/// `COUNT` queries; only the result rows cross into the envelope.
const LIVE_COUNTS_SQL: &str = "SELECT \
     COUNT(*) FILTER (WHERE s.group = 'backlog') AS backlog, \
     COUNT(*) FILTER (WHERE s.group = 'unstarted') AS unstarted, \
     COUNT(*) FILTER (WHERE s.group = 'started') AS started, \
     COUNT(*) FILTER (WHERE s.group = 'cancelled') AS cancelled, \
     COUNT(*) FILTER (WHERE s.group = 'completed') AS completed, \
     COUNT(*) AS total \
     FROM issues i \
     JOIN cycle_issues ci ON ci.issue_id = i.id AND ci.cycle_id = $3 AND ci.deleted_at IS NULL \
     JOIN workspaces w ON w.id = i.workspace_id AND w.slug = $1 \
     JOIN projects p ON p.id = i.project_id AND p.id = $2 AND p.archived_at IS NULL \
     LEFT JOIN states s ON s.id = i.state_id \
     WHERE i.deleted_at IS NULL AND i.archived_at IS NULL AND i.is_draft = FALSE \
     AND NOT (s.group = 'triage')";

// ---------------------------------------------------------------------------
// Envelope: the `or 0` rule + Python key order
// ---------------------------------------------------------------------------

/// `aggregate[k] or 0` (`base.py:769-773`): `None` and `0.0` both answer
/// int `0`; only nonzero sums stay floats.
fn or_zero(value: Option<f64>) -> Value {
    match value {
        Some(v) if v != 0.0 => json_float(v),
        _ => Value::Number(0.into()),
    }
}

/// `total` is NOT passed through `or 0` (`base.py:774`): the
/// `COALESCE` zero answers int `0`, a real zero sum answers `0.0`.
fn total_num(value: Option<f64>) -> Value {
    match value {
        None => Value::Number(0.into()),
        Some(v) => json_float(v),
    }
}

fn json_float(v: f64) -> Value {
    serde_json::Number::from_f64(v)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// Envelope in the exact Python literal key order (`base.py:767-783`).
#[allow(clippy::too_many_arguments)]
fn progress_envelope(estimates: [Option<f64>; 5], total: Option<f64>, counts: [i64; 6]) -> String {
    let mut out = Map::with_capacity(12);
    let keys = [
        "backlog_estimate_points",
        "unstarted_estimate_points",
        "started_estimate_points",
        "cancelled_estimate_points",
        "completed_estimate_points",
    ];
    for (key, value) in keys.iter().zip(estimates.iter()) {
        out.insert((*key).to_owned(), or_zero(*value));
    }
    out.insert("total_estimate_points".to_owned(), total_num(total));
    for (key, value) in [
        "backlog_issues",
        "total_issues",
        "completed_issues",
        "cancelled_issues",
        "started_issues",
        "unstarted_issues",
    ]
    .iter()
    .zip(counts.iter())
    {
        out.insert((*key).to_owned(), Value::Number((*value).into()));
    }
    serde_json::to_string(&Value::Object(out)).unwrap_or("null".to_owned())
}

/// Snapshot count lookup: `snapshot.get(key, 0)` (`base.py:713-718`).
fn snapshot_count(snapshot: &Map<String, Value>, key: &str) -> i64 {
    match snapshot.get(key) {
        Some(Value::Number(n)) => n
            .as_i64()
            .unwrap_or_else(|| n.as_f64().unwrap_or(0.0) as i64),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `CycleProgressEndpoint.get` (`base.py:660-783`).
async fn progress_get(
    State(state): State<AppState>,
    Path((slug, project_raw, cycle_raw)): Path<(String, String, String)>,
    Query(_query): Query<HashMap<String, String>>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    // `<uuid:cycle_id>` never matches a non-UUID tail in Django: proxy
    // so the routing 404 comes from Django byte-for-byte.
    let cycle_id: uuid::Uuid = match cycle_raw.parse() {
        Ok(id) => id,
        Err(_) => return Ok(crate::edge::proxy(State(state), req).await),
    };
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    // `<str:project_id>` reaches the view; the ORM UUID validation
    // raises `ValidationError` -> 400 (`app/views/base.py:126-130`).
    let project_id: uuid::Uuid = project_raw.parse().map_err(|_| Denial::BadValidation)?;
    let (workspace_role, project_role) = membership(&pool, &slug, &project_id, &user_id).await?;
    let row = gate_for("GET", PROGRESS_PATH).expect("progress route must have a gate");
    let facts = gate_facts(&slug, workspace_role, project_role, user_id);
    match decide_gate(&row.gate, &tenant_context(&slug), &facts) {
        GateOutcome::Allow => {}
        GateOutcome::Deny => return Err(Denial::Forbidden),
        GateOutcome::Unauthenticated => return Err(Denial::Unauthorized),
    }

    // Plain `Cycle.objects` manager: no `deleted_at` predicate (B1).
    // `NULL` snapshot decodes as `None` and reads falsy, like Python.
    let cycle: Option<(uuid::Uuid, Option<Value>)> = sqlx::query_as(
        r#"SELECT c.id, c.progress_snapshot FROM cycles c
           JOIN workspaces w ON w.id = c.workspace_id AND w.slug = $1
           WHERE c.project_id = $2 AND c.id = $3"#,
    )
    .bind(&slug)
    .bind(project_id)
    .bind(cycle_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((_id, snapshot)) = cycle else {
        return Err(Denial::NotFound);
    };
    let snapshot = snapshot.unwrap_or(Value::Null);

    /// One estimate-aggregate row: five per-group sums plus the total.
    type EstimateAgg = (
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
    );
    let agg: Option<EstimateAgg> = sqlx::query_as(&estimates_sql())
        .bind(&slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let (backlog, unstarted, started, cancelled, completed, total) = agg.unwrap_or_default();

    // `if cycle.progress_snapshot:` — truthiness of the stored dict.
    let counts: [i64; 6] = match &snapshot {
        Value::Object(map) if !map.is_empty() => [
            snapshot_count(map, "backlog_issues"),
            snapshot_count(map, "total_issues"),
            snapshot_count(map, "completed_issues"),
            snapshot_count(map, "cancelled_issues"),
            snapshot_count(map, "started_issues"),
            snapshot_count(map, "unstarted_issues"),
        ],
        _ => {
            let row: Option<(i64, i64, i64, i64, i64, i64)> = sqlx::query_as(LIVE_COUNTS_SQL)
                .bind(&slug)
                .bind(project_id)
                .bind(cycle_id)
                .fetch_optional(&pool)
                .await
                .map_err(|_| Denial::ServerError)?;
            let (b, u, s, c, d, t) = row.unwrap_or_default();
            // Envelope order is backlog, total, completed, cancelled,
            // started, unstarted (`base.py:775-781`).
            [b, t, d, c, s, u]
        }
    };

    Ok(json_response(
        StatusCode::OK,
        progress_envelope(
            [backlog, unstarted, started, cancelled, completed],
            total,
            counts,
        ),
    ))
}

/// [`AllowFacts`] for the GUEST progress gate over the fetched roles.
fn gate_facts(
    slug: &str,
    workspace_role: Option<i32>,
    project_role: Option<i32>,
    _user_id: uuid::Uuid,
) -> pidash_auth::permissions::allow::AllowFacts {
    use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
    use pidash_types::WorkspaceId;
    let allowed = [ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST];
    pidash_auth::permissions::allow::AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: workspace_role.is_some(),
        has_allowed_workspace_role: workspace_role.is_some_and(|r| allowed.contains(&r)),
        is_creator: false,
        has_allowed_project_role: project_role.is_some_and(|r| allowed.contains(&r)),
        is_project_member: project_role.is_some(),
        is_workspace_admin: workspace_role == Some(ROLE_ADMIN),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The golden vectors pinned against the real fixture file, so a
    /// fixture edit that changes the contract fails the build here.
    fn fixture() -> Value {
        let raw = include_str!("../../../../fixtures/app_cycles/progress.json");
        serde_json::from_str(raw).expect("progress fixture must parse")
    }

    #[test]
    fn envelope_key_order_matches_fixture_output_keys() {
        let body = progress_envelope([None; 5], None, [0; 6]);
        let parsed: Value = serde_json::from_str(&body).expect("envelope must parse");
        let order: Vec<&str> = parsed
            .as_object()
            .expect("envelope is an object")
            .keys()
            .map(String::as_str)
            .collect();
        let owned = fixture();
        let expected: Vec<&str> = owned["output_keys"]
            .as_array()
            .expect("output_keys is a list")
            .iter()
            .map(|v| v.as_str().expect("key is a string"))
            .collect();
        assert_eq!(order, expected);
    }

    #[test]
    fn or_zero_collapses_none_and_zero_to_int() {
        assert_eq!(or_zero(None), Value::Number(0.into()));
        assert_eq!(or_zero(Some(0.0)), Value::Number(0.into()));
        assert_eq!(or_zero(Some(-0.0)), Value::Number(0.into()));
        assert_eq!(
            or_zero(Some(5.0)),
            serde_json::Number::from_f64(5.0)
                .map(Value::Number)
                .unwrap()
        );
        assert_eq!(or_zero(Some(2.5)).to_string(), "2.5");
    }

    #[test]
    fn total_keeps_zero_float_but_coalesce_zero_is_int() {
        assert_eq!(total_num(None), Value::Number(0.into()));
        assert_eq!(total_num(Some(0.0)).to_string(), "0.0");
        assert_eq!(total_num(Some(14.5)).to_string(), "14.5");
    }

    #[test]
    fn snapshot_branch_renders_fixture_golden() {
        // F-C27-08 "snapshot branch": estimates live, counts from keys.
        let body = progress_envelope(
            [Some(5.0), Some(0.0), Some(0.0), Some(0.0), Some(13.0)],
            Some(18.0),
            [1, 3, 2, 0, 0, 0],
        );
        let golden = &fixture()["cases"][0];
        let parsed: Value = serde_json::from_str(&body).expect("envelope must parse");
        for (key, want) in golden["estimates"].as_object().expect("estimates") {
            assert_eq!(&parsed[key], want, "estimate key {key}");
        }
        for (key, want) in golden["issues"].as_object().expect("issues") {
            assert_eq!(&parsed[key], want, "issue key {key}");
        }
    }

    #[test]
    fn empty_cycle_renders_int_zeros() {
        // F-C27-08 "live-count branch, empty cycle": every value int 0.
        let body = progress_envelope([None; 5], None, [0; 6]);
        assert_eq!(
            body,
            r#"{"backlog_estimate_points":0,"unstarted_estimate_points":0,"started_estimate_points":0,"cancelled_estimate_points":0,"completed_estimate_points":0,"total_estimate_points":0,"backlog_issues":0,"total_issues":0,"completed_issues":0,"cancelled_issues":0,"started_issues":0,"unstarted_issues":0}"#
        );
    }

    #[test]
    fn snapshot_counts_default_missing_keys_to_zero() {
        let snap: Map<String, Value> = serde_json::from_str(r#"{"total_issues": 4}"#).expect("map");
        assert_eq!(snapshot_count(&snap, "total_issues"), 4);
        assert_eq!(snapshot_count(&snap, "backlog_issues"), 0);
        let empty = Map::new();
        assert_eq!(snapshot_count(&empty, "total_issues"), 0);
    }

    #[test]
    fn progress_gate_is_guest_and_denies_by_default() {
        use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
        let row = gate_for("GET", PROGRESS_PATH).expect("progress route must have a gate");
        let scope = tenant_context("acme");
        let allow = |role: Option<i32>| {
            let allowed = [ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST];
            let facts = pidash_auth::permissions::allow::AllowFacts {
                workspace: pidash_types::WorkspaceId::from("acme"),
                authenticated: role.is_some(),
                is_workspace_member: role.is_some(),
                has_allowed_workspace_role: role.is_some_and(|r| allowed.contains(&r)),
                is_creator: false,
                has_allowed_project_role: role.is_some_and(|r| allowed.contains(&r)),
                is_project_member: role.is_some(),
                is_workspace_admin: role == Some(ROLE_ADMIN),
            };
            decide_gate(&row.gate, &scope, &facts)
        };
        assert_eq!(allow(Some(ROLE_GUEST)), GateOutcome::Allow);
        assert_eq!(allow(Some(ROLE_MEMBER)), GateOutcome::Allow);
        assert_eq!(allow(Some(ROLE_ADMIN)), GateOutcome::Allow);
        assert_eq!(allow(None), GateOutcome::Unauthenticated);
    }

    #[test]
    fn denial_bodies_are_byte_exact() {
        assert_eq!(Denial::Unauthorized.status_and_body().1, ANON_BODY);
        assert_eq!(Denial::Forbidden.status_and_body().1, FORBIDDEN_BODY);
        assert_eq!(
            Denial::NotFound.status_and_body(),
            (StatusCode::NOT_FOUND, CYCLE_NOT_FOUND_BODY.to_owned())
        );
        assert_eq!(
            Denial::BadValidation.status_and_body(),
            (StatusCode::BAD_REQUEST, VALIDATION_BODY.to_owned())
        );
        let missing = fixture()["missing_cycle"].clone();
        assert_eq!(missing["status"], 404);
        assert_eq!(
            serde_json::to_string(&missing["body"]).expect("body"),
            CYCLE_NOT_FOUND_BODY
        );
    }

    #[test]
    fn sql_carries_bridge_scope_and_manager_excludes() {
        let sql = estimates_sql();
        for fragment in [
            "ci.cycle_id = $3",
            "ci.deleted_at IS NULL",
            "w.slug = $1",
            "p.id = $2",
            "p.archived_at IS NULL",
            "e.type = 'points'",
            "i.deleted_at IS NULL",
            "i.archived_at IS NULL",
            "i.is_draft = FALSE",
            "NOT (s.group = 'triage')",
            "CAST(ep.value AS DOUBLE PRECISION)",
        ] {
            assert!(
                sql.contains(fragment),
                "estimates SQL must carry {fragment}"
            );
        }
        assert!(LIVE_COUNTS_SQL.contains("FILTER (WHERE s.group = 'backlog')"));
        assert!(LIVE_COUNTS_SQL.contains("COUNT(*) AS total"));
    }
}
