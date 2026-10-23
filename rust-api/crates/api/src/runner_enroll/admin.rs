#![forbid(unsafe_code)]

//! Instance-admin runner list (PDASHOSS01-209).
//!
//! Ports `apps/api/pi_dash/runner/admin_views.py` (`InstanceRunnerListEndpoint`;
//! route `runner/admin_urls.py`, mounted under `/api/instances/runners/` by
//! `license/urls.py`): the read-only, cross-workspace support view of
//! per-runner `provisioning`, `runner_version` and
//! `dev_metadata.codex_version` (`.ai_design/managed_runner/design.md`
//! §15.1, §23 item 8).
//!
//! - Gate: `InstanceAdminPermission` via [`license::require_admin`] —
//!   anonymous (or the app cookie on an `instances` path) is 401, a
//!   non-admin the DRF-default 403.
//! - `?provisioning=` / `?status=`: present (even empty) and not a model
//!   choice is 400 `{"error":"invalid_provisioning"|"invalid_status",
//!   "detail":[sorted choices]}`; checked in that order.
//! - `?workspace=`: exact slug match, empty means no filter.
//! - `?page=`: `max(1, int(page))`, 1 on any `int()` failure; fixed
//!   `per=50`, `-created_at` order. Envelope `{"page","total","results"}`.
//!
//! Ported quirks (translate, don't redesign):
//!
//! * A page whose OFFSET leaves Postgres `bigint` range 500s: Django
//!   renders the slice as SQL literals and the database rejects them.
//! * A truthy non-object `dev_metadata` (`[1]`, `"x"`, `1`, `true`) 500s:
//!   `dev_metadata.get` raises `AttributeError` into the generic branch.
//! * Datetimes render with Python `isoformat()` (`+00:00`, microseconds
//!   only when nonzero) — the row builder calls `.isoformat()` directly.

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::runner_enroll::columns::enums::{
    RUNNER_PROVISIONING_CHOICES, RUNNER_STATUS_CHOICES,
};
use pidash_services::runner_runs::guards::py_int_saturated;

use crate::license::{self, json_response, query_last, Denial, QueryMap};
use crate::middleware::SessionHandle;
use crate::state::AppState;

/// Mount path (`license/urls.py` `path("runners/", …)`).
pub const RUNNERS_PATH: &str = "/api/instances/runners/";

/// `per = 50` (`admin_views.py`).
const PER_PAGE: i128 = 50;

/// `GET` serves from Rust; every other method proxies so Django's own
/// 405s (and DRF `OPTIONS` metadata) stay byte-for-byte.
pub fn routes() -> Router<AppState> {
    Router::new().route(RUNNERS_PATH, license::owned(get(list_runners), &["GET"]))
}

/// One `runner` row with its `select_related("workspace", "owner")` joins.
#[derive(Debug, sqlx::FromRow)]
struct RunnerRow {
    id: Uuid,
    name: String,
    workspace_slug: String,
    owner_email: String,
    host_label: String,
    provisioning: String,
    status: String,
    runner_version: String,
    dev_metadata: Option<Value>,
    last_heartbeat_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

/// Filters in the order the view applies them (and binds them).
#[derive(Debug, Default, PartialEq, Eq)]
struct Filters {
    provisioning: Option<String>,
    status: Option<String>,
    workspace: Option<String>,
}

/// An unknown choice value: the view's inline error code and the choices
/// it lists (sorted) in `detail`.
type InvalidChoice = (&'static str, &'static [&'static str]);

/// The view's inline 400 for an unknown choice value.
fn invalid_choice((error, choices): InvalidChoice) -> Response {
    let mut sorted: Vec<&str> = choices.to_vec();
    sorted.sort_unstable();
    let mut response = json_response(&json!({ "error": error, "detail": sorted }));
    *response.status_mut() = axum::http::StatusCode::BAD_REQUEST;
    response
}

/// Read and validate the filters (`admin_views.py`): `provisioning`, then
/// `status` — `is not None`, so a present-but-empty value is rejected —
/// then `workspace` (`if workspace:`).
fn parse_filters(query: &QueryMap) -> Result<Filters, InvalidChoice> {
    let provisioning = query_last(query, "provisioning");
    if let Some(value) = &provisioning {
        if !RUNNER_PROVISIONING_CHOICES.contains(&value.as_str()) {
            return Err(("invalid_provisioning", RUNNER_PROVISIONING_CHOICES));
        }
    }
    let status = query_last(query, "status");
    if let Some(value) = &status {
        if !RUNNER_STATUS_CHOICES.contains(&value.as_str()) {
            return Err(("invalid_status", RUNNER_STATUS_CHOICES));
        }
    }
    let workspace = query_last(query, "workspace").filter(|slug| !slug.is_empty());
    Ok(Filters {
        provisioning,
        status,
        workspace,
    })
}

/// `max(1, int(page))` falling back to 1 on `ValueError`, plus the slice's
/// OFFSET. `None` when the OFFSET leaves `bigint` range (Django's
/// literal-OFFSET query then fails in Postgres → generic 500).
fn page_window(raw: Option<&str>) -> Option<(i64, i64)> {
    let page = raw.and_then(py_int_saturated).unwrap_or(1).max(1);
    let offset = (page - 1).checked_mul(PER_PAGE)?;
    Some((i64::try_from(page).ok()?, i64::try_from(offset).ok()?))
}

/// `WHERE` clause over the joined aliases, binds numbered from `$1`.
fn where_sql(filters: &Filters) -> String {
    let mut parts = Vec::new();
    for (present, column) in [
        (filters.provisioning.is_some(), "r.provisioning"),
        (filters.status.is_some(), "r.status"),
        (filters.workspace.is_some(), "w.slug"),
    ] {
        if present {
            parts.push(format!("{column} = ${}", parts.len() + 1));
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", parts.join(" AND "))
    }
}

const FROM_SQL: &str = "FROM runner r INNER JOIN workspaces w ON (r.workspace_id = w.id) INNER JOIN users u ON (r.owner_id = u.id)";

fn page_sql(filters: &Filters, offset: i64) -> String {
    let mut sql = format!(
        "SELECT r.id AS id, r.name AS name, w.slug AS workspace_slug, u.email AS owner_email, r.host_label AS host_label, r.provisioning AS provisioning, r.status AS status, r.runner_version AS runner_version, r.dev_metadata AS dev_metadata, r.last_heartbeat_at AS last_heartbeat_at, r.created_at AS created_at, r.revoked_at AS revoked_at {FROM_SQL}{} ORDER BY r.created_at DESC LIMIT {PER_PAGE}",
        where_sql(filters)
    );
    if offset > 0 {
        sql.push_str(&format!(" OFFSET {offset}"));
    }
    sql
}

fn count_sql(filters: &Filters) -> String {
    format!("SELECT COUNT(*) {FROM_SQL}{}", where_sql(filters))
}

fn bind_filters<'q, O>(
    mut query: sqlx::query::QueryAs<'q, sqlx::Postgres, O, sqlx::postgres::PgArguments>,
    filters: &'q Filters,
) -> sqlx::query::QueryAs<'q, sqlx::Postgres, O, sqlx::postgres::PgArguments> {
    for value in [&filters.provisioning, &filters.status, &filters.workspace]
        .into_iter()
        .flatten()
    {
        query = query.bind(value.as_str());
    }
    query
}

/// Python `datetime.isoformat()` for a UTC instant: `+00:00` suffix (never
/// `Z`), microseconds only when nonzero.
fn iso(dt: &DateTime<Utc>) -> String {
    let format = if dt.timestamp_subsec_micros() == 0 {
        SecondsFormat::Secs
    } else {
        SecondsFormat::Micros
    };
    dt.to_rfc3339_opts(format, false)
}

fn iso_or_null(dt: Option<&DateTime<Utc>>) -> Value {
    dt.map_or(Value::Null, |dt| Value::String(iso(dt)))
}

/// Python truthiness of a decoded JSON value.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `(r.dev_metadata or {}).get("codex_version")`; `None` is the
/// `AttributeError` path (truthy non-dict).
fn codex_version(dev_metadata: Option<&Value>) -> Option<Value> {
    match dev_metadata {
        Some(Value::Object(map)) => Some(map.get("codex_version").cloned().unwrap_or(Value::Null)),
        Some(value) if truthy(value) => None,
        _ => Some(Value::Null),
    }
}

/// `_row` (`admin_views.py`), keys in source order.
fn render_row(row: &RunnerRow) -> Option<Value> {
    let mut out = Map::new();
    out.insert("id".into(), Value::String(row.id.to_string()));
    out.insert("name".into(), Value::String(row.name.clone()));
    out.insert(
        "workspace_slug".into(),
        Value::String(row.workspace_slug.clone()),
    );
    out.insert("owner_email".into(), Value::String(row.owner_email.clone()));
    out.insert("host_label".into(), Value::String(row.host_label.clone()));
    out.insert(
        "provisioning".into(),
        Value::String(row.provisioning.clone()),
    );
    out.insert("status".into(), Value::String(row.status.clone()));
    out.insert(
        "runner_version".into(),
        Value::String(row.runner_version.clone()),
    );
    out.insert(
        "codex_version".into(),
        codex_version(row.dev_metadata.as_ref())?,
    );
    out.insert(
        "last_heartbeat_at".into(),
        iso_or_null(row.last_heartbeat_at.as_ref()),
    );
    out.insert("created_at".into(), Value::String(iso(&row.created_at)));
    out.insert("revoked_at".into(), iso_or_null(row.revoked_at.as_ref()));
    Some(Value::Object(out))
}

fn pool_of(state: &AppState) -> Result<&PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// `GET /api/instances/runners/` (`InstanceRunnerListEndpoint.get`).
async fn list_runners(
    State(state): State<AppState>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    if let Err(denial) = license::require_admin(&state, extension).await {
        return denial.into_response();
    }
    let filters = match parse_filters(&query) {
        Ok(filters) => filters,
        Err(invalid) => return invalid_choice(invalid),
    };
    let Some((page, offset)) = page_window(query_last(&query, "page").as_deref()) else {
        return Denial::ServerError.into_response();
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // A NUL byte can never match a stored slug and the driver cannot bind
    // one; the ORM probe matches nothing (`license` slug-check precedent).
    let nul_slug = filters
        .workspace
        .as_deref()
        .is_some_and(|s| s.contains('\0'));

    let (rows, total): (Vec<RunnerRow>, i64) = if nul_slug {
        (Vec::new(), 0)
    } else {
        let page_query = page_sql(&filters, offset);
        let rows = match bind_filters(sqlx::query_as::<_, RunnerRow>(&page_query), &filters)
            .fetch_all(pool)
            .await
        {
            Ok(rows) => rows,
            Err(_) => return Denial::ServerError.into_response(),
        };
        let count_query = count_sql(&filters);
        let total = match bind_filters(sqlx::query_as::<_, (i64,)>(&count_query), &filters)
            .fetch_one(pool)
            .await
        {
            Ok((total,)) => total,
            Err(_) => return Denial::ServerError.into_response(),
        };
        (rows, total)
    };

    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        match render_row(row) {
            Some(value) => results.push(value),
            None => return Denial::ServerError.into_response(),
        }
    }
    json_response(&json!({ "page": page, "total": total, "results": results }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::license::OneOrMany;

    fn query(pairs: &[(&str, &str)]) -> QueryMap {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), OneOrMany::One((*v).to_owned())))
            .collect()
    }

    #[test]
    fn filters_accept_choices_and_skip_empty_workspace() {
        let filters = parse_filters(&query(&[
            ("provisioning", "desktop_bundled"),
            ("status", "online"),
            ("workspace", ""),
        ]))
        .expect("valid filters");
        assert_eq!(
            filters,
            Filters {
                provisioning: Some("desktop_bundled".into()),
                status: Some("online".into()),
                workspace: None,
            }
        );
    }

    #[test]
    fn empty_or_unknown_choice_is_rejected_provisioning_first() {
        for (pairs, expected) in [
            (vec![("provisioning", "")], "invalid_provisioning"),
            (
                vec![("provisioning", "bogus"), ("status", "bogus")],
                "invalid_provisioning",
            ),
            (vec![("status", "")], "invalid_status"),
            (vec![("status", "ONLINE")], "invalid_status"),
        ] {
            let (error, _) = parse_filters(&query(&pairs)).expect_err("rejected");
            assert_eq!(error, expected);
        }
    }

    #[test]
    fn invalid_choice_is_400_with_sorted_detail() {
        let response = invalid_choice(("invalid_status", RUNNER_STATUS_CHOICES));
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn page_window_follows_python_int() {
        assert_eq!(page_window(None), Some((1, 0)));
        assert_eq!(page_window(Some("")), Some((1, 0)));
        assert_eq!(page_window(Some("abc")), Some((1, 0)));
        assert_eq!(page_window(Some("2.0")), Some((1, 0)));
        assert_eq!(page_window(Some("-3")), Some((1, 0)));
        assert_eq!(page_window(Some("0")), Some((1, 0)));
        assert_eq!(page_window(Some(" 3 ")), Some((3, 100)));
        assert_eq!(page_window(Some("1_0")), Some((10, 450)));
        assert_eq!(page_window(Some("+2")), Some((2, 50)));
        // OFFSET past bigint: Django's query fails in Postgres (500).
        assert_eq!(page_window(Some("999999999999999999999")), None);
    }

    #[test]
    fn where_sql_numbers_binds_in_filter_order() {
        let filters = Filters {
            provisioning: None,
            status: Some("busy".into()),
            workspace: Some("ws".into()),
        };
        assert_eq!(where_sql(&filters), " WHERE r.status = $1 AND w.slug = $2");
        assert_eq!(where_sql(&Filters::default()), "");
        assert!(page_sql(&Filters::default(), 0).ends_with("ORDER BY r.created_at DESC LIMIT 50"));
        assert!(page_sql(&Filters::default(), 100).ends_with("LIMIT 50 OFFSET 100"));
    }

    #[test]
    fn codex_version_mirrors_or_dict_get() {
        assert_eq!(codex_version(None), Some(Value::Null));
        assert_eq!(codex_version(Some(&json!({}))), Some(Value::Null));
        assert_eq!(codex_version(Some(&json!([]))), Some(Value::Null));
        assert_eq!(codex_version(Some(&json!(""))), Some(Value::Null));
        assert_eq!(codex_version(Some(&json!(0))), Some(Value::Null));
        assert_eq!(codex_version(Some(&json!(false))), Some(Value::Null));
        assert_eq!(
            codex_version(Some(&json!({"codex_version": "rust-v0.153.4"}))),
            Some(json!("rust-v0.153.4"))
        );
        assert_eq!(codex_version(Some(&json!({"other": 1}))), Some(Value::Null));
        // Truthy non-dict: `AttributeError` → generic 500.
        assert_eq!(codex_version(Some(&json!([1]))), None);
        assert_eq!(codex_version(Some(&json!("x"))), None);
        assert_eq!(codex_version(Some(&json!(true))), None);
    }

    #[test]
    fn iso_renders_python_isoformat() {
        let whole = DateTime::parse_from_rfc3339("2026-10-23T01:02:03Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(iso(&whole), "2026-10-23T01:02:03+00:00");
        let micros = DateTime::parse_from_rfc3339("2026-10-23T01:02:03.000450Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(iso(&micros), "2026-10-23T01:02:03.000450+00:00");
    }
}
