//! Instance-admin runner list: params, SQL and row shape (PDASHOSS01-303).
//!
//! Ports `apps/api/pi_dash/runner/admin_views.py` (`InstanceRunnerListEndpoint`,
//! mounted at `/api/instances/runners/` by `license/urls.py`). The HTTP shell
//! (session auth, `InstanceAdminPermission`, row fetching) lives in
//! `pidash_api::runner_admin`; everything here is pure.
//!
//! - `provisioning` / `status`: `QueryDict.get` values. A *present* value
//!   (empty included) must be a known choice, else 400
//!   `{"error":"invalid_provisioning"|"invalid_status","detail":[sorted]}`.
//! - `workspace`: truthy slug filters on `workspace__slug`; empty is ignored.
//! - `page`: `max(1, int(raw))`, `1` on `ValueError`; 50 rows per page.
//! - Order is `-created_at`; the envelope is `{"page","total","results"}`.
//! - Row datetimes use Python `isoformat()` (`+00:00`, microseconds only
//!   when nonzero), not the DRF `Z` kernel — the view formats them itself.

use chrono::{DateTime, SecondsFormat, Utc};
use pidash_db::runner_enroll::columns::enums::{
    RUNNER_PROVISIONING_CHOICES, RUNNER_STATUS_CHOICES,
};
use serde_json::{json, Value};
use uuid::Uuid;

/// `per = 50` (`admin_views.py`).
pub const PAGE_SIZE: i64 = 50;

/// A rejected filter value: the exact 400 body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterError {
    InvalidProvisioning,
    InvalidStatus,
}

impl FilterError {
    /// `{"error": …, "detail": sorted(<Choices>.values)}`.
    pub fn body(self) -> Value {
        let (error, choices) = match self {
            FilterError::InvalidProvisioning => {
                ("invalid_provisioning", RUNNER_PROVISIONING_CHOICES)
            }
            FilterError::InvalidStatus => ("invalid_status", RUNNER_STATUS_CHOICES),
        };
        let mut detail: Vec<&str> = choices.to_vec();
        detail.sort_unstable();
        json!({ "error": error, "detail": detail })
    }
}

/// Validated list parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListParams {
    pub provisioning: Option<String>,
    pub status: Option<String>,
    pub workspace_slug: Option<String>,
    pub page: i64,
}

impl ListParams {
    /// Validate the raw `QueryDict.get` values in view order: provisioning,
    /// then status (the first failure answers), then workspace and page.
    pub fn parse(
        provisioning: Option<&str>,
        status: Option<&str>,
        workspace: Option<&str>,
        page: Option<&str>,
    ) -> Result<Self, FilterError> {
        if let Some(value) = provisioning {
            if !RUNNER_PROVISIONING_CHOICES.contains(&value) {
                return Err(FilterError::InvalidProvisioning);
            }
        }
        if let Some(value) = status {
            if !RUNNER_STATUS_CHOICES.contains(&value) {
                return Err(FilterError::InvalidStatus);
            }
        }
        // `max(1, int(raw))` with the `1` fallback is exactly the runs
        // paginator's `page` rule (CPython `int()` acceptance included).
        let (page, _) = crate::runner_runs::guards::parse_pagination(page, None);
        Ok(Self {
            provisioning: provisioning.map(str::to_owned),
            status: status.map(str::to_owned),
            workspace_slug: workspace.filter(|slug| !slug.is_empty()).map(str::to_owned),
            page,
        })
    }

    /// `qs[start : start + per]` OFFSET, or `None` when it leaves bigint
    /// range (Postgres rejects the literal, so Django answers its 500).
    pub fn offset(&self) -> Option<i64> {
        (self.page - 1).checked_mul(PAGE_SIZE)
    }

    /// `WHERE` clause over `runner` (`r`) / `workspaces` (`w`) with `$n`
    /// binds in provisioning, status, workspace order; empty when unfiltered.
    pub fn where_sql(&self) -> String {
        let mut parts = Vec::new();
        for (present, column) in [
            (self.provisioning.is_some(), "r.provisioning"),
            (self.status.is_some(), "r.status"),
            (self.workspace_slug.is_some(), "w.slug"),
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

    /// The bind values matching [`Self::where_sql`], in order.
    pub fn binds(&self) -> Vec<&str> {
        [&self.provisioning, &self.status, &self.workspace_slug]
            .into_iter()
            .filter_map(|value| value.as_deref())
            .collect()
    }
}

/// `select_related("workspace", "owner")`: both FKs are NOT NULL, so the
/// joins are inner and the related default managers never apply (a
/// soft-deleted workspace still renders its slug).
pub const FROM_SQL: &str = "FROM runner r INNER JOIN workspaces w ON (r.workspace_id = w.id) INNER JOIN users u ON (r.owner_id = u.id)";

/// The projected columns for [`RunnerRow`].
pub const SELECT_SQL: &str = "SELECT r.id AS id, r.name AS name, w.slug AS workspace_slug, u.email AS owner_email, r.host_label AS host_label, r.provisioning AS provisioning, r.status AS status, r.runner_version AS runner_version, r.dev_metadata AS dev_metadata, r.last_heartbeat_at AS last_heartbeat_at, r.created_at AS created_at, r.revoked_at AS revoked_at";

/// One runner with its joined workspace slug and owner email.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RunnerRow {
    pub id: Uuid,
    pub name: String,
    pub workspace_slug: String,
    /// `User.email` is nullable.
    pub owner_email: Option<String>,
    pub host_label: String,
    pub provisioning: String,
    pub status: String,
    pub runner_version: String,
    pub dev_metadata: Value,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// `(r.dev_metadata or {}).get(...)` on a truthy non-dict raises
/// `AttributeError` into the generic 500.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonDictMetadata;

/// Python `datetime.isoformat()` for a UTC instant.
pub fn iso(dt: &DateTime<Utc>) -> String {
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

/// `_row` (`admin_views.py`), keys in dict order.
pub fn render_row(row: &RunnerRow) -> Result<Value, NonDictMetadata> {
    let codex_version = match &row.dev_metadata {
        Value::Object(map) => map.get("codex_version").cloned().unwrap_or(Value::Null),
        other if truthy(other) => return Err(NonDictMetadata),
        _ => Value::Null,
    };
    Ok(json!({
        "id": row.id.to_string(),
        "name": row.name,
        "workspace_slug": row.workspace_slug,
        "owner_email": row.owner_email,
        "host_label": row.host_label,
        "provisioning": row.provisioning,
        "status": row.status,
        "runner_version": row.runner_version,
        "codex_version": codex_version,
        "last_heartbeat_at": iso_or_null(row.last_heartbeat_at.as_ref()),
        "created_at": iso(&row.created_at),
        "revoked_at": iso_or_null(row.revoked_at.as_ref()),
    }))
}

/// DRF `JSONRenderer` output: compact JSON with U+2028/U+2029 escaped.
pub fn render_body(body: &Value) -> String {
    body.to_string()
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn parse(
        provisioning: Option<&str>,
        status: Option<&str>,
        workspace: Option<&str>,
        page: Option<&str>,
    ) -> Result<ListParams, FilterError> {
        ListParams::parse(provisioning, status, workspace, page)
    }

    #[test]
    fn empty_or_unknown_filters_are_rejected() {
        assert_eq!(
            parse(Some(""), None, None, None),
            Err(FilterError::InvalidProvisioning)
        );
        assert_eq!(
            parse(Some("bogus"), None, None, None),
            Err(FilterError::InvalidProvisioning)
        );
        assert_eq!(
            parse(None, Some(""), None, None),
            Err(FilterError::InvalidStatus)
        );
        assert_eq!(
            parse(None, Some("ONLINE"), None, None),
            Err(FilterError::InvalidStatus)
        );
        // Provisioning is checked first.
        assert_eq!(
            parse(Some("x"), Some("y"), None, None),
            Err(FilterError::InvalidProvisioning)
        );
    }

    #[test]
    fn error_bodies_list_sorted_choices() {
        assert_eq!(
            FilterError::InvalidProvisioning.body().to_string(),
            r#"{"error":"invalid_provisioning","detail":["desktop_bundled","manual"]}"#
        );
        assert_eq!(
            FilterError::InvalidStatus.body().to_string(),
            r#"{"error":"invalid_status","detail":["busy","offline","online","revoked"]}"#
        );
    }

    #[test]
    fn page_follows_python_int_with_fallback() {
        let page = |raw| parse(None, None, None, raw).unwrap().page;
        assert_eq!(page(None), 1);
        assert_eq!(page(Some("abc")), 1);
        assert_eq!(page(Some("")), 1);
        assert_eq!(page(Some("0")), 1);
        assert_eq!(page(Some("-3")), 1);
        assert_eq!(page(Some(" 2 ")), 2);
        assert_eq!(page(Some("1_0")), 10);
        assert_eq!(page(Some("2.0")), 1);
    }

    #[test]
    fn offset_overflow_is_none() {
        let params = parse(None, None, None, Some("3")).unwrap();
        assert_eq!(params.offset(), Some(100));
        let huge = parse(None, None, None, Some("99999999999999999999999")).unwrap();
        assert_eq!(huge.offset(), None);
    }

    #[test]
    fn where_and_binds_follow_filter_order() {
        let all = parse(Some("manual"), Some("online"), Some("ws"), None).unwrap();
        assert_eq!(
            all.where_sql(),
            " WHERE r.provisioning = $1 AND r.status = $2 AND w.slug = $3"
        );
        assert_eq!(all.binds(), vec!["manual", "online", "ws"]);
        let ws_only = parse(None, None, Some("ws"), None).unwrap();
        assert_eq!(ws_only.where_sql(), " WHERE w.slug = $1");
        let none = parse(None, None, Some(""), None).unwrap();
        assert_eq!(none.where_sql(), "");
        assert!(none.binds().is_empty());
    }

    fn row(dev_metadata: Value) -> RunnerRow {
        RunnerRow {
            id: Uuid::nil(),
            name: "r\u{2028}1".to_owned(),
            workspace_slug: "ws".to_owned(),
            owner_email: Some("a@b.c".to_owned()),
            host_label: "".to_owned(),
            provisioning: "manual".to_owned(),
            status: "offline".to_owned(),
            runner_version: "1.2.3".to_owned(),
            dev_metadata,
            last_heartbeat_at: None,
            created_at: Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap(),
            revoked_at: Some(
                Utc.with_ymd_and_hms(2026, 10, 2, 1, 2, 3).unwrap()
                    + chrono::Duration::microseconds(789),
            ),
        }
    }

    #[test]
    fn row_shape_matches_python_dict() {
        let rendered = render_row(&row(json!({"codex_version": "rust-v0.1"}))).unwrap();
        assert_eq!(
            render_body(&rendered),
            r#"{"id":"00000000-0000-0000-0000-000000000000","name":"r\u20281","workspace_slug":"ws","owner_email":"a@b.c","host_label":"","provisioning":"manual","status":"offline","runner_version":"1.2.3","codex_version":"rust-v0.1","last_heartbeat_at":null,"created_at":"2026-10-01T12:00:00+00:00","revoked_at":"2026-10-02T01:02:03.000789+00:00"}"#
        );
    }

    #[test]
    fn codex_version_falls_back_to_null() {
        for metadata in [
            json!({}),
            json!(null),
            json!([]),
            json!(""),
            json!(0),
            json!(false),
        ] {
            let rendered = render_row(&row(metadata)).unwrap();
            assert_eq!(rendered["codex_version"], Value::Null);
        }
        for metadata in [json!([1]), json!("x"), json!(1), json!(true)] {
            assert_eq!(render_row(&row(metadata)), Err(NonDictMetadata));
        }
    }
}
