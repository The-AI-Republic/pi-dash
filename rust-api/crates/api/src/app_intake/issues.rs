//! Intake-issue detail handlers (D-32, stage 5): `partial_update`,
//! `retrieve`, `destroy` (`app/views/intake/base.py:329-566`) over the
//! `intake-issues/<pk>/` + `inbox-issues/<pk>/` aliases.
//!
//! The `<pk>` is the **issue** id (every lookup keys `issue_id=pk`,
//! `:334-339,379-396,474-498,506-530,551-557`); the bridge row is found
//! through it, never addressed directly.
//!
//! `partial_update` (`:329-500`): `skip_activity` pop; member /
//! workspace-admin gates; GUEST `issue_data` narrowing to the 3-key
//! subset; the dual issue + intake serializers validated **before**
//! either saves; the migration-update silent path (`skip_activity` and a
//! top-level `description_html` together skip the issue-branch emits);
//! re-fetch with the `label_ids` / `assignee_ids` annotations plus
//! `IntakeIssueDetailSerializer`, 200.
//!
//! `retrieve` (`:503-547`): annotated re-fetch; GUEST 403 unless the
//! creator; `IntakeIssueDetailSerializer`, 200.
//!
//! `destroy` (`:550-566`): a bridge status in `[-2,-1,0,2]` also deletes
//! the `Issue`; deletes the intake issue; 204.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * BUG-partial-intake-missing (`:333-339`): the lookup passes the
//!   `Intake` *instance* as `intake_id`, so a missing intake compiles to
//!   `IS NULL` and answers 404 — while `retrieve` (`:529`)
//!   dereferences `intake_id.id` and answers 500 for the same state.
//!   Ported as written.
//! * BUG-destroy-dangling-issue (`:562-563`): `.first()` may be `None`
//!   and `.delete()` is unconditional, so a missing issue row answers
//!   500, never 404. Ported as written.
//! * QUIRK-complexity-message (`serializers/issue.py:185-197`): the
//!   custom `validate_complexity_score` message is unreachable — DRF runs
//!   the model Min/Max validators first — so out-of-range scores answer
//!   the model messages. Ported as observed.
//! * QUIRK-description-binary-dead (`serializers/issue.py:333-336`): the
//!   `description_binary` check tests `attrs`, but the model
//!   `BinaryField` maps to a read-only serializer field, so the key can
//!   never be present. The input key is ignored.
//! * QUIRK-pop-mutates (`:330,371`): `skip_activity` and `issue` are
//!   popped off `request.data`, so the intake-branch `requested_data`
//!   dump never contains them (the create path pops nothing).
//! * v1 description version on create: enqueue-only, like Django
//!   (`base.py:287-292` `.delay(is_creating=True)`). No synchronous row
//!   insert: PIDASHCONV-500's QUIRK-create-sync-v1 (option (a1)) is gone
//!   for good — PIDASHCONV-911 removed it for replay fidelity, PIDASHCONV-939
//!   restored it because the contract-rust Rust run was workerless, and
//!   PIDASHCONV-983 removed it again durably: the Rust run now starts
//!   `pidash-api worker` for WORKER_SUITES (see WORKER_SUITES.txt), and the
//!   versions tests poll until the row lands (`_wait_for_versions`).

use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_db::app_intake::queries;
use pidash_db::app_pages::strip::ml_strip_tags;
use pidash_db::issue_filters::{
    issue_filters_get, FilterValue, IssueFilterError, ISSUE_FILTER_KEYS,
};
use pidash_db::tasks_ticker::IssueAgentTicker;
use pidash_services::app_intake::permissions as guards;
use pidash_services::app_intake::tasks as intake_tasks;
use pidash_services::app_issues::serializers_create::managed_unavailable_detail;
use pidash_services::app_issues::serializers_create::HAS_ACTIVE_RUN_SQL;
use pidash_services::app_issues::serializers_detail::{
    agent_status_to_representation, agent_ticker_to_representation, serialize_iso_datetime,
    AgentLiveStateRow, AgentRunDetailRow, AgentTickerView, ACTIVE_AGENT_RUN_SQL,
    AGENT_RUN_COUNT_SQL, LATEST_AGENT_RUN_SQL,
};
use pidash_services::dispatch::admission::{managed_runner_availability, LlmProfile};
use pidash_services::dispatch::policy::{
    cloud_agent_is_configured, managed_runner_is_enabled, UserFlags,
};
use pidash_services::orchestration::clock::ProjectClockPolicy;
use pidash_types::dispatch::{AgentExecutorKind, ManagedRunnerReason};
use pidash_types::orchestration::StateRef;

use crate::app_issues::handlers_core::{
    assistant_has_api_key, enrolled_runner, online_runner, paused_runner,
};
use crate::app_issues::resolve_gate;
use crate::middleware::SessionHandle;

use super::{
    actor, enqueue_message, enqueue_soft_delete, guard_denial, guards_error, guest_view_all,
    intake_id_for, is_issue_creator, json_truthy, load_membership, parse_body, parse_id, pool_of,
    query_last, raw_json_response, resolve_project_id, Denial, QueryMap,
};
use crate::serializer::render_datetime_in;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// One `issues` row with every column the handlers read or render.
#[derive(Debug, Clone)]
pub struct IssueRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    pub description_json: serde_json::Value,
    pub priority: String,
    pub start_date: Option<chrono::NaiveDate>,
    pub target_date: Option<chrono::NaiveDate>,
    pub sequence_id: i32,
    pub created_by_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub project_id: Uuid,
    pub state_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub description_html: String,
    pub description_stripped: Option<String>,
    pub completed_at: Option<DateTime<Utc>>,
    pub sort_order: f64,
    pub point: Option<i32>,
    pub archived_at: Option<chrono::NaiveDate>,
    pub is_draft: bool,
    pub external_id: Option<String>,
    pub external_source: Option<String>,
    pub description_binary: Option<Vec<u8>>,
    pub estimate_point_id: Option<Uuid>,
    pub type_id: Option<Uuid>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub git_work_branch: String,
    pub assigned_pod_id: Option<Uuid>,
    pub created_via: Option<String>,
    pub agent_executor: Option<String>,
    pub complexity_score: i32,
}

impl IssueRow {
    #[allow(clippy::too_many_lines)]
    pub fn get(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            name: row.try_get("name")?,
            description_json: row.try_get("description_json")?,
            priority: row.try_get("priority")?,
            start_date: row.try_get("start_date")?,
            target_date: row.try_get("target_date")?,
            sequence_id: row.try_get("sequence_id")?,
            created_by_id: row.try_get("created_by_id")?,
            parent_id: row.try_get("parent_id")?,
            project_id: row.try_get("project_id")?,
            state_id: row.try_get("state_id")?,
            updated_by_id: row.try_get("updated_by_id")?,
            workspace_id: row.try_get("workspace_id")?,
            description_html: row.try_get("description_html")?,
            description_stripped: row.try_get("description_stripped")?,
            completed_at: row.try_get("completed_at")?,
            sort_order: row.try_get("sort_order")?,
            point: row.try_get("point")?,
            archived_at: row.try_get("archived_at")?,
            is_draft: row.try_get("is_draft")?,
            external_id: row.try_get("external_id")?,
            external_source: row.try_get("external_source")?,
            description_binary: row.try_get("description_binary")?,
            estimate_point_id: row.try_get("estimate_point_id")?,
            type_id: row.try_get("type_id")?,
            deleted_at: row.try_get("deleted_at")?,
            git_work_branch: row.try_get("git_work_branch")?,
            assigned_pod_id: row.try_get("assigned_pod_id")?,
            created_via: row.try_get("created_via")?,
            agent_executor: row.try_get("agent_executor")?,
            complexity_score: row.try_get("complexity_score")?,
        })
    }
}

/// One `intake_issues` row.
#[derive(Debug, Clone)]
pub struct IntakeIssueRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub status: i32,
    pub snoozed_till: Option<DateTime<Utc>>,
    pub source: Option<String>,
    pub created_by_id: Option<Uuid>,
    pub duplicate_to_id: Option<Uuid>,
    pub intake_id: Uuid,
    pub issue_id: Uuid,
    pub project_id: Uuid,
    pub updated_by_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub external_id: Option<String>,
    pub external_source: Option<String>,
    pub extra: serde_json::Value,
    pub source_email: Option<String>,
}

impl IntakeIssueRow {
    pub fn get(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            status: row.try_get("status")?,
            snoozed_till: row.try_get("snoozed_till")?,
            source: row.try_get("source")?,
            created_by_id: row.try_get("created_by_id")?,
            duplicate_to_id: row.try_get("duplicate_to_id")?,
            intake_id: row.try_get("intake_id")?,
            issue_id: row.try_get("issue_id")?,
            project_id: row.try_get("project_id")?,
            updated_by_id: row.try_get("updated_by_id")?,
            workspace_id: row.try_get("workspace_id")?,
            external_id: row.try_get("external_id")?,
            external_source: row.try_get("external_source")?,
            extra: row.try_get("extra")?,
            source_email: row.try_get("source_email")?,
        })
    }
}

// ---------------------------------------------------------------------------
// Fetches (same WHERE semantics as the queries layer; explicit columns so
// both joined rows decode by name)
// ---------------------------------------------------------------------------

const INTAKE_ISSUE_COLS: &str = concat!(
    "t.id AS ii_id, t.created_at AS ii_created_at, t.updated_at AS ii_updated_at, ",
    "t.status AS ii_status, t.snoozed_till AS ii_snoozed_till, t.source AS ii_source, ",
    "t.created_by_id AS ii_created_by_id, t.duplicate_to_id AS ii_duplicate_to_id, ",
    "t.intake_id AS ii_intake_id, t.issue_id AS ii_issue_id, t.project_id AS ii_project_id, ",
    "t.updated_by_id AS ii_updated_by_id, t.workspace_id AS ii_workspace_id"
);

const ISSUE_COLS: &str = concat!(
    "i.id, i.created_at, i.updated_at, i.name, i.description_json, i.priority, ",
    "i.start_date, i.target_date, i.sequence_id, i.created_by_id, i.parent_id, ",
    "i.project_id, i.state_id, i.updated_by_id, i.workspace_id, i.description_html, ",
    "i.description_stripped, i.completed_at, i.sort_order, i.point, i.archived_at, ",
    "i.is_draft, i.external_id, i.external_source, i.description_binary, ",
    "i.estimate_point_id, i.type_id, i.deleted_at, i.git_work_branch, ",
    "i.assigned_pod_id, i.created_via, i.agent_executor, i.complexity_score"
);

fn intake_issue_from(row: &sqlx::postgres::PgRow) -> Result<IntakeIssueRow, sqlx::Error> {
    Ok(IntakeIssueRow {
        id: row.try_get("ii_id")?,
        created_at: row.try_get("ii_created_at")?,
        updated_at: row.try_get("ii_updated_at")?,
        status: row.try_get("ii_status")?,
        snoozed_till: row.try_get("ii_snoozed_till")?,
        source: row.try_get("ii_source")?,
        created_by_id: row.try_get("ii_created_by_id")?,
        duplicate_to_id: row.try_get("ii_duplicate_to_id")?,
        intake_id: row.try_get("ii_intake_id")?,
        issue_id: row.try_get("ii_issue_id")?,
        project_id: row.try_get("ii_project_id")?,
        updated_by_id: row.try_get("ii_updated_by_id")?,
        workspace_id: row.try_get("ii_workspace_id")?,
        external_id: None,
        external_source: None,
        extra: serde_json::Value::Object(Default::default()),
        source_email: None,
    })
}

/// `.get(issue_id=pk, workspace__slug, project_id, intake_id)`
/// (`:333-339`, `:551-557`): the `Intake` instance compiles to the same
/// pk comparison. `intake_id=None` compiles to `IS NULL` (BUG-intake…
/// partial form) — callers pass `None` through to reproduce the 404.
pub async fn lookup_intake_issue(
    pool: &PgPool,
    issue_id: &Uuid,
    slug: &str,
    project_id: &Uuid,
    intake_id: Option<Uuid>,
) -> Result<Option<IntakeIssueRow>, Denial> {
    let row = sqlx::query(
        r#"SELECT t.* FROM "intake_issues" t
           INNER JOIN "workspaces" w ON (t.workspace_id = w.id)
           WHERE (t.issue_id = $1 AND w.slug = $2 AND t.project_id = $3
                  AND t.intake_id IS NOT DISTINCT FROM $4 AND t.deleted_at IS NULL)"#,
    )
    .bind(issue_id)
    .bind(slug)
    .bind(project_id)
    .bind(intake_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| IntakeIssueRow::get(&row).map_err(|_| Denial::ServerError))
        .transpose()
}

/// Array annotations exactly like Django renders them (`:379-396`,
/// `:474-498`, `:506-530`): `LEFT OUTER JOIN` to the through tables
/// only (`issue_labels` / `issue_assignees` — never the target tables),
/// `ARRAY_AGG(DISTINCT through.fk)` with the through-`deleted_at` guard,
/// `GROUP BY` both ids. The partial-update issue form and the
/// re-fetch/retrieve forms share this guard (through `deleted_at`, no
/// `is_active`); the call-site asymmetry the queries layer records
/// concerns the *other* queryset forms, not these three.
const LABEL_IDS_ANNOTATION: &str = r#"COALESCE(ARRAY_AGG(DISTINCT "il"."label_id") FILTER (WHERE ("il"."label_id" IS NOT NULL AND "il"."deleted_at" IS NULL)), '{}')"#;
const ASSIGNEE_IDS_ANNOTATION: &str = r#"COALESCE(ARRAY_AGG(DISTINCT "ia"."assignee_id") FILTER (WHERE ("ia"."assignee_id" IS NOT NULL AND "ia"."deleted_at" IS NULL)), '{}')"#;

/// The `Issue` annotate for the issue branch (`:379-396`): the issue
/// row plus the `label_ids` / `assignee_ids` annotations.
pub async fn fetch_issue_annotated(
    pool: &PgPool,
    issue_id: &Uuid,
    project_id: &Uuid,
    slug: &str,
) -> Result<Option<(IssueRow, Vec<Uuid>, Vec<Uuid>)>, Denial> {
    let sql = format!(
        r#"SELECT {cols}, {labels} AS "label_ids", {assignees} AS "assignee_ids" FROM "issues" i
           INNER JOIN "workspaces" w ON (i.workspace_id = w.id)
           LEFT OUTER JOIN "issue_labels" il ON (i.id = il.issue_id)
           LEFT OUTER JOIN "issue_assignees" ia ON (i.id = ia.issue_id)
           WHERE (i.id = $1 AND i.project_id = $2 AND w.slug = $3 AND i.deleted_at IS NULL)
           GROUP BY i.id"#,
        cols = ISSUE_COLS,
        labels = LABEL_IDS_ANNOTATION,
        assignees = ASSIGNEE_IDS_ANNOTATION,
    );
    let row = sqlx::query(&sql)
        .bind(issue_id)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    match row {
        None => Ok(None),
        Some(row) => {
            let issue = IssueRow::get(&row).map_err(|_| Denial::ServerError)?;
            let label_ids: Vec<Uuid> = row.try_get("label_ids").map_err(|_| Denial::ServerError)?;
            let assignee_ids: Vec<Uuid> = row
                .try_get("assignee_ids")
                .map_err(|_| Denial::ServerError)?;
            Ok(Some((issue, label_ids, assignee_ids)))
        }
    }
}

/// The re-fetch projection (`:474-498`, `:506-530`): bridge + issue
/// rows with the `label_ids` / refetch-guard `assignee_ids` annotations.
pub async fn fetch_detail(
    pool: &PgPool,
    intake_id: &Uuid,
    issue_id: &Uuid,
    project_id: &Uuid,
) -> Result<Option<(IntakeIssueRow, IssueRow, Vec<Uuid>, Vec<Uuid>)>, Denial> {
    let sql = format!(
        r#"SELECT {ii}, {cols}, {labels} AS "label_ids", {assignees} AS "assignee_ids"
           FROM "intake_issues" t INNER JOIN "issues" i ON (t.issue_id = i.id)
           LEFT OUTER JOIN "issue_labels" il ON (i.id = il.issue_id)
           LEFT OUTER JOIN "issue_assignees" ia ON (i.id = ia.issue_id)
           WHERE (t.intake_id = $1 AND t.issue_id = $2 AND t.project_id = $3
                  AND t.deleted_at IS NULL)
           GROUP BY t.id, i.id"#,
        ii = INTAKE_ISSUE_COLS,
        cols = ISSUE_COLS,
        labels = LABEL_IDS_ANNOTATION,
        assignees = ASSIGNEE_IDS_ANNOTATION,
    );
    let row = sqlx::query(&sql)
        .bind(intake_id)
        .bind(issue_id)
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    match row {
        None => Ok(None),
        Some(row) => {
            let bridge = intake_issue_from(&row).map_err(|_| Denial::ServerError)?;
            let issue = IssueRow::get(&row).map_err(|_| Denial::ServerError)?;
            let label_ids: Vec<Uuid> = row.try_get("label_ids").map_err(|_| Denial::ServerError)?;
            let assignee_ids: Vec<Uuid> = row
                .try_get("assignee_ids")
                .map_err(|_| Denial::ServerError)?;
            Ok(Some((bridge, issue, label_ids, assignee_ids)))
        }
    }
}

/// `destroy`'s conditional issue lookup (`:562`): `.filter(...).first()`.
pub async fn fetch_issue_for_destroy(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<Option<IssueRow>, Denial> {
    let row = sqlx::query(
        r#"SELECT i.* FROM "issues" i
           INNER JOIN "workspaces" w ON (i.workspace_id = w.id)
           WHERE (w.slug = $1 AND i.project_id = $2 AND i.id = $3 AND i.deleted_at IS NULL)
           LIMIT 1"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(issue_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| IssueRow::get(&row).map_err(|_| Denial::ServerError))
        .transpose()
}

// ---------------------------------------------------------------------------
// DRF field validation (partial=True: absent keys are skipped, never
// required; read-only and unknown keys are ignored)
// ---------------------------------------------------------------------------

/// Field errors in DRF's `{"field": ["message"]}` shape.
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

/// DRF `IntegerField.to_internal_value`: bools fail; ints pass; floats
/// with integral value pass; numeric strings parse; everything else
/// fails.
fn validate_integer(
    errors: &mut FieldErrors,
    field: &str,
    value: &serde_json::Value,
    allow_null: bool,
) -> Option<i64> {
    if value.is_null() {
        if !allow_null {
            push_error(errors, field, "This field may not be null.".to_owned());
        }
        return None;
    }
    match value {
        serde_json::Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                Some(int)
            } else if let Some(uint) = number.as_u64() {
                i64::try_from(uint).ok().or_else(|| {
                    push_error(errors, field, "A valid integer is required.".to_owned());
                    None
                })
            } else if let Some(float) = number.as_f64() {
                if float.fract() == 0.0 && float >= i64::MIN as f64 && float <= i64::MAX as f64 {
                    Some(float as i64)
                } else {
                    push_error(errors, field, "A valid integer is required.".to_owned());
                    None
                }
            } else {
                push_error(errors, field, "A valid integer is required.".to_owned());
                None
            }
        }
        serde_json::Value::String(text) => match text.trim().parse::<i64>() {
            Ok(int) => Some(int),
            Err(_) => {
                push_error(errors, field, "A valid integer is required.".to_owned());
                None
            }
        },
        _ => {
            push_error(errors, field, "A valid integer is required.".to_owned());
            None
        }
    }
}

/// DRF `FloatField.to_internal_value`.
fn validate_float(
    errors: &mut FieldErrors,
    field: &str,
    value: &serde_json::Value,
    allow_null: bool,
) -> Option<f64> {
    if value.is_null() {
        if !allow_null {
            push_error(errors, field, "This field may not be null.".to_owned());
        }
        return None;
    }
    match value {
        serde_json::Value::Number(number) => number.as_f64().or_else(|| {
            push_error(errors, field, "A valid number is required.".to_owned());
            None
        }),
        serde_json::Value::String(text) => match text.trim().parse::<f64>() {
            Ok(float) => Some(float),
            Err(_) => {
                push_error(errors, field, "A valid number is required.".to_owned());
                None
            }
        },
        _ => {
            push_error(errors, field, "A valid number is required.".to_owned());
            None
        }
    }
}

/// DRF `BooleanField.to_internal_value`: the JSON booleans plus the
/// historical string spellings.
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
            if number.as_i64() == Some(1) || number.as_u64() == Some(1) {
                Some(true)
            } else if number.as_i64() == Some(0) || number.as_u64() == Some(0) {
                Some(false)
            } else {
                push_error(errors, field, "Must be a valid boolean.".to_owned());
                None
            }
        }
        serde_json::Value::String(text) => match text.trim().to_lowercase().as_str() {
            "true" | "t" | "1" => Some(true),
            "false" | "f" | "0" => Some(false),
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

/// DRF `DateField.to_internal_value` (`YYYY-MM-DD`).
fn validate_date(
    errors: &mut FieldErrors,
    field: &str,
    value: &serde_json::Value,
    allow_null: bool,
) -> Option<chrono::NaiveDate> {
    if value.is_null() {
        if !allow_null {
            push_error(errors, field, "This field may not be null.".to_owned());
        }
        return None;
    }
    let text = match value {
        serde_json::Value::String(text) => text.clone(),
        _ => {
            push_error(errors, field, date_format_message());
            return None;
        }
    };
    match chrono::NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d") {
        Ok(date) => Some(date),
        Err(_) => {
            push_error(errors, field, date_format_message());
            None
        }
    }
}

fn date_format_message() -> String {
    "Date has wrong format. Use one of these formats instead: YYYY-MM-DD.".to_owned()
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
    let raw = match value {
        serde_json::Value::String(text) => text,
        _ => {
            push_error(errors, field, datetime_format_message());
            return None;
        }
    };
    let text = raw.trim().to_string();
    if let Ok(date) = chrono::NaiveDate::parse_from_str(&text, "%Y-%m-%d") {
        return Some(date.and_hms_opt(0, 0, 0).expect("midnight").and_utc());
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&text) {
        return Some(dt.with_timezone(&Utc));
    }
    // DRF's `strptime(value, 'iso-8601')` fallthrough (`to_internal_value`
    // runs it when `parse_datetime` returns `None` — PIDASHCONV-773):
    // the literal matches case-insensitively and yields naive
    // 1900-01-01, read as UTC like the date arm above. Checked on the RAW
    // text — Django's `strptime` sees the value before any strip (this
    // port's trim is a pre-existing quirk), so a padded literal still
    // fails. Exact match (probed); ASCII-only (765 unicode-gap family).
    if raw.eq_ignore_ascii_case("iso-8601") {
        let naive = chrono::NaiveDate::from_ymd_opt(1900, 1, 1)
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .expect("1900-01-01 valid");
        return Some(naive.and_utc());
    }
    // DRF also accepts the `+HHMM`/`+HH:MM` offsets and fractional
    // forms RFC 3339 covers; anything else is the format error.
    push_error(errors, field, datetime_format_message());
    None
}

fn datetime_format_message() -> String {
    "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]."
        .to_owned()
}

/// DRF `ChoiceField.to_internal_value` over integer choices: the value
/// renders with straight quotes (`"7" is not a valid choice.`).
fn validate_int_choice(
    errors: &mut FieldErrors,
    field: &str,
    value: &serde_json::Value,
    choices: &[i64],
    allow_null: bool,
) -> Option<i64> {
    if value.is_null() {
        if !allow_null {
            push_error(errors, field, "This field may not be null.".to_owned());
        }
        return None;
    }
    // JSON booleans compare equal to 0/1 in Python (`True == 1`), so a
    // boolean that matches a choice passes, like DRF over the same data.
    let candidate = match value {
        serde_json::Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int
            } else if let Some(uint) = number.as_u64() {
                match i64::try_from(uint) {
                    Ok(int) => int,
                    Err(_) => {
                        push_error(errors, field, format!("\"{value}\" is not a valid choice."));
                        return None;
                    }
                }
            } else {
                push_error(errors, field, format!("\"{value}\" is not a valid choice."));
                return None;
            }
        }
        serde_json::Value::Bool(true) => 1,
        serde_json::Value::Bool(false) => 0,
        serde_json::Value::String(text) => match text.trim().parse::<i64>() {
            Ok(int) => int,
            Err(_) => {
                push_error(errors, field, format!("\"{text}\" is not a valid choice."));
                return None;
            }
        },
        _ => {
            push_error(
                errors,
                field,
                format!("\"{}\" is not a valid choice.", super::python_dumps(value)),
            );
            return None;
        }
    };
    if choices.contains(&candidate) {
        Some(candidate)
    } else {
        let display = match value {
            serde_json::Value::String(text) => text.clone(),
            _ => candidate.to_string(),
        };
        push_error(
            errors,
            field,
            format!("\"{display}\" is not a valid choice."),
        );
        None
    }
}

/// DRF `ChoiceField` over string choices (priority, agent executors).
fn validate_str_choice(
    errors: &mut FieldErrors,
    field: &str,
    value: &serde_json::Value,
    choices: &[&str],
    allow_null: bool,
) -> Option<String> {
    if value.is_null() {
        if !allow_null {
            push_error(errors, field, "This field may not be null.".to_owned());
        }
        return None;
    }
    // Non-strings never match a string choice (DRF renders the raw
    // value); numbers stringify through `CharField` first, so an int
    // reads as its decimal text here.
    let text = match value {
        serde_json::Value::String(text) => text.trim().to_string(),
        serde_json::Value::Number(number) => number.to_string(),
        _ => {
            push_error(errors, field, "Not a valid string.".to_owned());
            return None;
        }
    };
    if choices.contains(&text.as_str()) {
        Some(text)
    } else {
        push_error(errors, field, format!("\"{text}\" is not a valid choice."));
        None
    }
}

/// DRF `UUIDField.to_internal_value`: the curly-quote message
/// (`"\u201cxyz\u201d is not a valid UUID."`), verified live.
fn validate_uuid_field(
    errors: &mut FieldErrors,
    field: &str,
    value: &serde_json::Value,
    allow_null: bool,
) -> Option<Uuid> {
    if value.is_null() {
        if !allow_null {
            push_error(errors, field, "This field may not be null.".to_owned());
        }
        return None;
    }
    let text = match value {
        serde_json::Value::String(text) => text.trim().to_string(),
        _ => {
            push_error(errors, field, "Not a valid string.".to_owned());
            return None;
        }
    };
    match text.parse::<Uuid>() {
        Ok(id) => Some(id),
        Err(_) => {
            push_error(
                errors,
                field,
                format!("\u{201c}{text}\u{201d} is not a valid UUID."),
            );
            None
        }
    }
}

/// DRF `PrimaryKeyRelatedField.to_internal_value` over a UUID pk:
/// malformed ids fail like the UUID field; well-formed ids must name a
/// live row in the field queryset.
pub async fn validate_pk(
    pool: &PgPool,
    errors: &mut FieldErrors,
    field: &str,
    value: &serde_json::Value,
    queryset_sql: &str,
    allow_null: bool,
) -> Option<Uuid> {
    let id = validate_uuid_field(errors, field, value, allow_null)?;
    if value.is_null() {
        return None;
    }
    let row: Option<(Uuid,)> = sqlx::query_as(queryset_sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ())
        .ok()?;
    if row.is_none() {
        push_error(
            errors,
            field,
            format!("Invalid pk \"{id}\" - object does not exist."),
        );
        return None;
    }
    Some(id)
}

/// Row-existence probes for the FK fields. Queryset notes:
/// * `state_id`: `State.all_state_objects` — the plain manager, so
///   soft-deleted rows still resolve here (the project check in
///   `validate()` rejects them next).
/// * `parent_id` / `estimate_point_id` / `duplicate_to`: the default
///   soft-delete-scoped managers.
fn state_any_sql() -> &'static str {
    r#"SELECT id FROM states WHERE id = $1"#
}

fn issue_live_sql() -> &'static str {
    r#"SELECT id FROM issues WHERE id = $1 AND deleted_at IS NULL"#
}

/// `duplicate_to` is an *auto-generated* FK field
/// (`serializers/intake.py:27-41` declares only `issue`), so DRF binds
/// the model's `_default_manager` — `Issue.issue_objects`, which hides
/// triage states, archived rows, drafts and archived projects
/// (`db/models/issue.py:96-108`). A triage issue (every intake issue!)
/// as `duplicate_to` answers `does_not_exist`, verified live.
fn duplicate_to_live_sql() -> &'static str {
    r#"SELECT i.id FROM issues i
       LEFT JOIN states st ON st.id = i.state_id
       LEFT JOIN projects p ON p.id = i.project_id
       WHERE i.id = $1 AND i.deleted_at IS NULL
         AND (st."group" IS NULL OR st."group" != 'triage')
         AND i.archived_at IS NULL AND NOT i.is_draft
         AND p.archived_at IS NULL"#
}

fn estimate_point_live_sql() -> &'static str {
    r#"SELECT id FROM estimate_points WHERE id = $1 AND deleted_at IS NULL"#
}

fn issue_type_live_sql() -> &'static str {
    r#"SELECT id FROM issue_types WHERE id = $1 AND deleted_at IS NULL"#
}

// ---------------------------------------------------------------------------
// Issue field validation (`IssueCreateSerializer`, partial)
// ---------------------------------------------------------------------------

/// Validated scalar updates for the issue branch. `None` fields were
/// absent (partial) or explicitly null on a nullable column.
#[derive(Debug, Default)]
pub struct ValidatedIssue {
    pub name: Option<String>,
    pub description_html: Option<String>,
    pub description_json: Option<serde_json::Value>,
    pub description_stripped: Option<Option<String>>,
    pub priority: Option<String>,
    pub complexity_score: Option<i64>,
    pub start_date: Option<Option<chrono::NaiveDate>>,
    pub target_date: Option<Option<chrono::NaiveDate>>,
    pub point: Option<Option<i64>>,
    pub sort_order: Option<f64>,
    pub completed_at: Option<Option<DateTime<Utc>>>,
    pub archived_at: Option<Option<chrono::NaiveDate>>,
    pub is_draft: Option<bool>,
    pub external_source: Option<Option<String>>,
    pub external_id: Option<Option<String>>,
    pub sequence_id: Option<i64>,
    pub state_id: Option<Option<Uuid>>,
    pub parent_id: Option<Option<Uuid>>,
    pub estimate_point_id: Option<Option<Uuid>>,
    pub type_id: Option<Option<Uuid>>,
    pub assigned_pod_id: Option<Option<Uuid>>,
    pub agent_executor: Option<Option<String>>,
    pub git_work_branch: Option<String>,
    pub created_via: Option<Option<String>>,
    pub assignee_ids: Option<Vec<Uuid>>,
    pub label_ids: Option<Vec<Uuid>>,
}

const ISSUE_PRIORITIES: &[&str] = &["low", "medium", "high", "urgent", "none"];
const AGENT_EXECUTORS: &[&str] = &["local_runner", "managed_runner", "cloud_agent"];

/// Field-level validation for the issue branch, in serializer field
/// order. Unknown and read-only keys (`id`, `project`, `workspace`,
/// `created_by`, `updated_by`, `created_at`, `updated_at`,
/// `description_binary`, `workpad`, `labels`, `assignees`) are ignored,
/// like DRF.
pub async fn validate_issue_fields(
    pool: &PgPool,
    input: &serde_json::Map<String, serde_json::Value>,
    errors: &mut FieldErrors,
) -> ValidatedIssue {
    let mut out = ValidatedIssue::default();
    if let Some(value) = input.get("name") {
        out.name = validate_char(errors, "name", value, 255, false, false);
    }
    if let Some(value) = input.get("description_html") {
        out.description_html =
            validate_char(errors, "description_html", value, usize::MAX, true, false);
    }
    if let Some(value) = input.get("description_json") {
        if value.is_null() {
            push_error(
                errors,
                "description_json",
                "This field may not be null.".to_owned(),
            );
        } else {
            out.description_json = Some(value.clone());
        }
    }
    if let Some(value) = input.get("description_stripped") {
        if value.is_null() {
            out.description_stripped = Some(None);
        } else {
            out.description_stripped = validate_char(
                errors,
                "description_stripped",
                value,
                usize::MAX,
                true,
                false,
            )
            .map(Some);
        }
    }
    if let Some(value) = input.get("priority") {
        out.priority = validate_str_choice(errors, "priority", value, ISSUE_PRIORITIES, false);
    }
    if let Some(value) = input.get("complexity_score") {
        // Model Min/Max validators run before the custom message, so
        // only the model wording is reachable (QUIRK-complexity-message).
        match validate_integer(errors, "complexity_score", value, false) {
            Some(score) if !(0..=10).contains(&score) => {
                if score < 0 {
                    push_error(
                        errors,
                        "complexity_score",
                        "Ensure this value is greater than or equal to 0.".to_owned(),
                    );
                } else {
                    push_error(
                        errors,
                        "complexity_score",
                        "Ensure this value is less than or equal to 10.".to_owned(),
                    );
                }
            }
            Some(score) => out.complexity_score = Some(score),
            None => {}
        }
    }
    if let Some(value) = input.get("start_date") {
        if value.is_null() {
            out.start_date = Some(None);
        } else {
            out.start_date = validate_date(errors, "start_date", value, false).map(Some);
        }
    }
    if let Some(value) = input.get("target_date") {
        if value.is_null() {
            out.target_date = Some(None);
        } else {
            out.target_date = validate_date(errors, "target_date", value, false).map(Some);
        }
    }
    if let Some(value) = input.get("point") {
        if value.is_null() {
            out.point = Some(None);
        } else {
            match validate_integer(errors, "point", value, false) {
                Some(point) if !(0..=12).contains(&point) => {
                    if point < 0 {
                        push_error(
                            errors,
                            "point",
                            "Ensure this value is greater than or equal to 0.".to_owned(),
                        );
                    } else {
                        push_error(
                            errors,
                            "point",
                            "Ensure this value is less than or equal to 12.".to_owned(),
                        );
                    }
                }
                Some(point) => out.point = Some(Some(point)),
                None => {}
            }
        }
    }
    if let Some(value) = input.get("sort_order") {
        out.sort_order = validate_float(errors, "sort_order", value, false);
    }
    if let Some(value) = input.get("completed_at") {
        if value.is_null() {
            out.completed_at = Some(None);
        } else {
            out.completed_at = validate_datetime(errors, "completed_at", value, false).map(Some);
        }
    }
    if let Some(value) = input.get("archived_at") {
        if value.is_null() {
            out.archived_at = Some(None);
        } else {
            out.archived_at = validate_date(errors, "archived_at", value, false).map(Some);
        }
    }
    if let Some(value) = input.get("is_draft") {
        out.is_draft = validate_boolean(errors, "is_draft", value, false);
    }
    if let Some(value) = input.get("external_source") {
        if value.is_null() {
            out.external_source = Some(None);
        } else {
            out.external_source =
                validate_char(errors, "external_source", value, 255, true, false).map(Some);
        }
    }
    if let Some(value) = input.get("external_id") {
        if value.is_null() {
            out.external_id = Some(None);
        } else {
            out.external_id =
                validate_char(errors, "external_id", value, 255, true, false).map(Some);
        }
    }
    if let Some(value) = input.get("sequence_id") {
        out.sequence_id = validate_integer(errors, "sequence_id", value, false);
    }
    if let Some(value) = input.get("state_id") {
        if value.is_null() {
            out.state_id = Some(None);
        } else if let Some(id) =
            validate_pk(pool, errors, "state_id", value, state_any_sql(), false).await
        {
            out.state_id = Some(Some(id));
        }
    }
    if let Some(value) = input.get("parent_id") {
        if value.is_null() {
            out.parent_id = Some(None);
        } else if let Some(id) =
            validate_pk(pool, errors, "parent_id", value, issue_live_sql(), false).await
        {
            out.parent_id = Some(Some(id));
        }
    }
    if let Some(value) = input.get("estimate_point_id") {
        if value.is_null() {
            out.estimate_point_id = Some(None);
        } else if let Some(id) = validate_pk(
            pool,
            errors,
            "estimate_point_id",
            value,
            estimate_point_live_sql(),
            false,
        )
        .await
        {
            out.estimate_point_id = Some(Some(id));
        }
    }
    if let Some(value) = input.get("type") {
        if value.is_null() {
            out.type_id = Some(None);
        } else if let Some(id) =
            validate_pk(pool, errors, "type", value, issue_type_live_sql(), false).await
        {
            out.type_id = Some(Some(id));
        }
    }
    if let Some(value) = input.get("assigned_pod_id") {
        if value.is_null() {
            out.assigned_pod_id = Some(None);
        } else if let Some(id) = validate_uuid_field(errors, "assigned_pod_id", value, false) {
            // `Pod.all_objects`: tombstoned rows still resolve
            // here; `validate()` reports them next.
            let row: Option<(Uuid,)> = sqlx::query_as(r#"SELECT id FROM pod WHERE id = $1"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .unwrap_or(None);
            if row.is_none() {
                push_error(
                    errors,
                    "assigned_pod_id",
                    format!("Invalid pk \"{id}\" - object does not exist."),
                );
            } else {
                out.assigned_pod_id = Some(Some(id));
            }
        }
    }
    if let Some(value) = input.get("agent_executor") {
        if value.is_null() {
            out.agent_executor = Some(None);
        } else {
            out.agent_executor =
                validate_str_choice(errors, "agent_executor", value, AGENT_EXECUTORS, false)
                    .map(Some);
        }
    }
    if let Some(value) = input.get("git_work_branch") {
        match validate_char(errors, "git_work_branch", value, 128, true, false) {
            Some(branch)
                if branch
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-')) =>
            {
                out.git_work_branch = Some(branch);
            }
            Some(_) => {
                push_error(
                    errors,
                    "git_work_branch",
                    "Branch name may contain only letters, numbers, and . _ / -".to_owned(),
                );
            }
            None => {}
        }
    }
    if let Some(value) = input.get("created_via") {
        if value.is_null() {
            out.created_via = Some(None);
        } else {
            out.created_via =
                validate_char(errors, "created_via", value, 32, true, false).map(Some);
        }
    }
    if let Some(value) = input.get("assignee_ids") {
        out.assignee_ids =
            validate_pk_list(pool, errors, "assignee_ids", value, user_live_sql()).await;
    }
    if let Some(value) = input.get("label_ids") {
        out.label_ids = validate_pk_list(pool, errors, "label_ids", value, label_live_sql()).await;
    }
    // `labels` / `assignees` (the auto M2M twins) are read-only on
    // `IssueCreateSerializer`: M2M fields with a custom through map
    // read-only, so DRF silently drops the keys. Ignore them here too.
    out
}

fn user_live_sql() -> &'static str {
    r#"SELECT id FROM users WHERE id = $1"#
}

fn label_live_sql() -> &'static str {
    r#"SELECT id FROM labels WHERE id = $1 AND deleted_at IS NULL"#
}

/// DRF `ListField(child=PrimaryKeyRelatedField)`: non-lists fail with
/// the type message; null fails; each item resolves like `validate_pk`.
pub async fn validate_pk_list(
    pool: &PgPool,
    errors: &mut FieldErrors,
    field: &str,
    value: &serde_json::Value,
    queryset_sql: &str,
) -> Option<Vec<Uuid>> {
    if value.is_null() {
        push_error(errors, field, "This field may not be null.".to_owned());
        return None;
    }
    let items = match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(_) => {
            push_error(
                errors,
                field,
                "Expected a list of items but got type \"dict\".".to_owned(),
            );
            return None;
        }
        _ => {
            let kind = match value {
                serde_json::Value::String(_) => "str",
                serde_json::Value::Number(_) => "int",
                serde_json::Value::Bool(_) => "bool",
                _ => "dict",
            };
            push_error(
                errors,
                field,
                format!("Expected a list of items but got type \"{kind}\"."),
            );
            return None;
        }
    };
    let mut ids = Vec::with_capacity(items.len());
    for item in items {
        match validate_pk(pool, errors, field, item, queryset_sql, false).await {
            Some(id) => ids.push(id),
            None => return None,
        }
    }
    Some(ids)
}

// ---------------------------------------------------------------------------
// `IssueCreateSerializer.validate` (object level)
// ---------------------------------------------------------------------------

const SYNC_LOCKED_MESSAGE: &str = "This field is synced from a Git provider and is read-only. Unbind the project's repository to edit.";

/// `validate()` outcome: filtered m2m id lists plus the sanitized HTML.
pub struct IssueObjectValidation {
    pub assignee_ids: Option<Vec<Uuid>>,
    pub label_ids: Option<Vec<Uuid>>,
    pub description_html: Option<String>,
}

/// Whether the stored issue is actively synced (`_issue_is_actively_synced`,
/// `serializers/issue.py:63-76`): empty `external_source` short-circuits
/// before any query.
pub async fn is_synced(pool: &PgPool, issue: &IssueRow) -> Result<bool, Denial> {
    let Some(source) = issue.external_source.as_deref() else {
        return Ok(false);
    };
    if source.is_empty() {
        return Ok(false);
    }
    // No `is_synced` annotation rides these handlers, so the sync tables
    // decide.
    let git: Option<(Uuid,)> =
        sqlx::query_as(r#"SELECT issue_id FROM git_issue_syncs WHERE issue_id = $1 LIMIT 1"#)
            .bind(issue.id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    if git.is_some() {
        return Ok(true);
    }
    let github: Option<(Uuid,)> =
        sqlx::query_as(r#"SELECT issue_id FROM github_issue_syncs WHERE issue_id = $1 LIMIT 1"#)
            .bind(issue.id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(github.is_some())
}

/// Table names for the sync probes above live behind Django model
/// names; resolve them defensively: a missing table reads as "not
/// synced" (plain projects never create these tables' rows, and the
/// contract tenants never carry sync state).
pub async fn is_synced_safe(pool: &PgPool, issue: &IssueRow) -> bool {
    is_synced(pool, issue).await.unwrap_or(false)
}

/// Whether the issue has a non-terminal agent run
/// (`Issue.has_active_run`, `db/models/issue.py:232-250`). Shares the
/// canonical `agent_run` existence check with the app-issues create path.
pub async fn has_active_run(pool: &PgPool, issue_id: &Uuid) -> Result<bool, Denial> {
    let row: Option<(i32,)> = sqlx::query_as(HAS_ACTIVE_RUN_SQL)
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// Object-level validation, in `validate()` order: sync lock, dates,
/// pod, executor, HTML sanitize, assignee/label filtering, state /
/// parent / estimate checks. First error wins — every `raise` in
/// `validate()` stops validation (`serializers/issue.py:206-385`) —
/// so each check returns as soon as it pushes. Field errors and
/// non-field errors share the DRF body; the caller answers 400 when
/// either is non-empty.
#[allow(clippy::too_many_arguments)]
pub async fn validate_issue_object(
    pool: &PgPool,
    settings: &pidash_db::config::Settings,
    project_id: &Uuid,
    workspace_id: &Uuid,
    viewer_id: &Uuid,
    issue: &IssueRow,
    validated: &ValidatedIssue,
    input: &serde_json::Map<String, serde_json::Value>,
    errors: &mut FieldErrors,
    non_field: &mut Vec<String>,
) -> Result<IssueObjectValidation, Denial> {
    // The caller answers 400 and ignores the outcome whenever an
    // error was pushed, so early exits return it empty.
    let empty = || IssueObjectValidation {
        assignee_ids: None,
        label_ids: None,
        description_html: None,
    };
    // Read-only lock for actively-synced issues: only changed values
    // block (`serializers/issue.py:210-229`).
    if is_synced_safe(pool, issue).await {
        let mut blocked: Vec<&str> = Vec::new();
        if let Some(name) = validated.name.as_deref() {
            if name != issue.name {
                blocked.push("name");
            }
        }
        if let Some(html) = validated.description_html.as_deref() {
            if html != issue.description_html {
                blocked.push("description_html");
            }
        }
        if let Some(json) = validated.description_json.as_ref() {
            if json != &issue.description_json {
                blocked.push("description_json");
            }
        }
        if let Some(stripped) = validated.description_stripped.as_ref() {
            let current = issue
                .description_stripped
                .clone()
                .map(serde_json::Value::String);
            let next = stripped.clone().map(serde_json::Value::String);
            if next != current {
                blocked.push("description_stripped");
            }
        }
        // One `raise` with a dict: every blocked key lands together.
        for field in &blocked {
            push_error(errors, field, SYNC_LOCKED_MESSAGE.to_owned());
        }
        if !blocked.is_empty() {
            return Ok(empty());
        }
    }

    if let (Some(Some(start)), Some(Some(target))) = (
        validated.start_date.as_ref(),
        validated.target_date.as_ref(),
    ) {
        if start > target {
            non_field.push("Start date cannot exceed target date".to_owned());
            return Ok(empty());
        }
    }

    if let Some(pod) = validated.assigned_pod_id.as_ref() {
        if validate_pod(pool, project_id, issue, *pod, errors, non_field).await? {
            return Ok(empty());
        }
    }

    if let Some(executor) = validated.agent_executor.as_ref() {
        if validate_executor(
            pool,
            settings,
            project_id,
            workspace_id,
            viewer_id,
            issue,
            executor.clone(),
            errors,
            non_field,
        )
        .await?
        {
            return Ok(empty());
        }
    }

    // HTML sanitize (`serializers/issue.py:326-332`): truthy values
    // only; failures answer `{"error": [...]}`.
    let mut sanitized_html: Option<String> = None;
    if let Some(html) = validated.description_html.clone() {
        if !html.is_empty() {
            match crate::space::sanitize::sanitize_html(&html) {
                crate::space::sanitize::Sanitize::Clean(clean) => {
                    sanitized_html = Some(clean);
                }
                crate::space::sanitize::Sanitize::Invalid => {
                    push_error(errors, "error", "html content is not valid".to_owned());
                    return Ok(empty());
                }
            }
        }
    }

    // Assignees narrow to active project members, role >= 15
    // (`serializers/issue.py:339-345`); labels to project labels
    // (`:348-355`).
    let mut assignee_ids = validated.assignee_ids.clone();
    if let Some(ids) = assignee_ids.as_ref() {
        if !ids.is_empty() {
            let rows: Vec<(Uuid,)> = sqlx::query_as(
                r#"SELECT pm.member_id FROM project_members pm
                   WHERE pm.project_id = $1 AND pm.role >= 15 AND pm.is_active
                     AND pm.member_id = ANY($2) AND pm.deleted_at IS NULL"#,
            )
            .bind(project_id)
            .bind(ids)
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            assignee_ids = Some(rows.into_iter().map(|row| row.0).collect());
        }
    }
    let mut label_ids = validated.label_ids.clone();
    if let Some(ids) = label_ids.as_ref() {
        if !ids.is_empty() {
            let rows: Vec<(Uuid,)> = sqlx::query_as(
                r#"SELECT l.id FROM labels l
                   WHERE l.project_id = $1 AND l.id = ANY($2) AND l.deleted_at IS NULL"#,
            )
            .bind(project_id)
            .bind(ids)
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            label_ids = Some(rows.into_iter().map(|row| row.0).collect());
        }
    }

    // State / parent / estimate project checks
    // (`serializers/issue.py:357-379`).
    if let Some(Some(state_id)) = validated.state_id.as_ref() {
        // `allow_triage_state` is always true on this path, so the
        // triage manager (triage + live) and the default manager
        // (non-triage + live) jointly cover every live project state.
        let row: Option<(Uuid,)> = sqlx::query_as(
            r#"SELECT s.id FROM states s
               WHERE s.project_id = $1 AND s.id = $2 AND s.deleted_at IS NULL"#,
        )
        .bind(project_id)
        .bind(state_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        if row.is_none() {
            non_field.push("State is not valid please pass a valid state_id".to_owned());
            return Ok(empty());
        }
        let _ = input;
    }
    if let Some(Some(parent_id)) = validated.parent_id.as_ref() {
        let row: Option<(Uuid,)> = sqlx::query_as(
            r#"SELECT id FROM issues WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL"#,
        )
        .bind(project_id)
        .bind(parent_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        if row.is_none() {
            non_field.push("Parent is not valid issue_id please pass a valid issue_id".to_owned());
            return Ok(empty());
        }
    }
    if let Some(Some(estimate_id)) = validated.estimate_point_id.as_ref() {
        let row: Option<(Uuid,)> = sqlx::query_as(
            r#"SELECT id FROM estimate_points WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL"#,
        )
        .bind(project_id)
        .bind(estimate_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        if row.is_none() {
            non_field.push(
                "Estimate point is not valid please pass a valid estimate_point_id".to_owned(),
            );
            return Ok(empty());
        }
    }

    Ok(IssueObjectValidation {
        assignee_ids,
        label_ids,
        description_html: sanitized_html,
    })
}

/// Pod checks (`serializers/issue.py:240-259`): project match, tombstone,
/// mid-flight reassignment. First error wins, like the sequential
/// `raise`s; returns whether an error was pushed.
async fn validate_pod(
    pool: &PgPool,
    project_id: &Uuid,
    issue: &IssueRow,
    pod: Option<Uuid>,
    errors: &mut FieldErrors,
    _non_field: &mut Vec<String>,
) -> Result<bool, Denial> {
    let Some(pod_id) = pod else { return Ok(false) };
    let row: Option<(Uuid, Option<chrono::DateTime<Utc>>)> =
        sqlx::query_as(r#"SELECT project_id, deleted_at FROM pod WHERE id = $1"#)
            .bind(pod_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some((pod_project, pod_deleted)) = row else {
        return Ok(false);
    };
    if pod_project != *project_id {
        push_error(
            errors,
            "assigned_pod_id",
            "pod is in a different project".to_owned(),
        );
        return Ok(true);
    }
    if pod_deleted.is_some() {
        push_error(errors, "assigned_pod_id", "pod has been deleted".to_owned());
        return Ok(true);
    }
    if issue.assigned_pod_id.is_some()
        && issue.assigned_pod_id != Some(pod_id)
        && has_active_run(pool, &issue.id).await?
    {
        push_error(
            errors,
            "assigned_pod_id",
            "cannot reassign pod while the issue has an active run".to_owned(),
        );
        return Ok(true);
    }
    Ok(false)
}

/// Executor checks (`serializers/issue.py:275-321`): the
/// settings-backed cloud/managed gates with Django's messages
/// (mirroring `app_issues` step 4), and the mid-flight rule for
/// explicit null too (`:314-321` sits outside
/// `if executor is not None`). First error wins, like the sequential
/// `raise`s; returns whether an error was pushed.
#[allow(clippy::too_many_arguments)]
async fn validate_executor(
    pool: &PgPool,
    settings: &pidash_db::config::Settings,
    project_id: &Uuid,
    workspace_id: &Uuid,
    viewer_id: &Uuid,
    issue: &IssueRow,
    executor: Option<String>,
    errors: &mut FieldErrors,
    _non_field: &mut Vec<String>,
) -> Result<bool, Denial> {
    if let Some(executor) = executor.as_deref() {
        // Field level (`AGENT_EXECUTORS`) already rejected unknown
        // values with the `ChoiceField` message, so only the three
        // kinds arrive (`:280-281` is unreachable past the field).
        match AgentExecutorKind::from_value(executor) {
            Some(AgentExecutorKind::CloudAgent)
                if !cloud_agent_is_configured(&settings.cloud_agent) =>
            {
                push_error(
                    errors,
                    "agent_executor",
                    "Pi Dash Cloud Agent is not available on this instance".to_owned(),
                );
                return Ok(true);
            }
            Some(AgentExecutorKind::ManagedRunner) => {
                // `self.instance.project` (`:295`): always resolved on
                // update — the tenant project is the issue's project.
                let mut llm_profile = LlmProfile {
                    available: false,
                    reason_code: ManagedRunnerReason::LLM_CONFIG_MISSING.to_owned(),
                };
                let mut enrolled_exists = false;
                let mut online_exists = false;
                let mut paused_exists = false;
                // `managed_runner_availability` gates in fixed order;
                // each probe runs only when reached (mirrors
                // `app_issues::handlers_core`).
                let mut viewer: Option<UserFlags> = None;
                if managed_runner_is_enabled(&settings.managed_runner) {
                    let flags: Option<(bool, bool)> =
                        sqlx::query_as(r#"SELECT is_active, is_bot FROM users WHERE id = $1"#)
                            .bind(viewer_id)
                            .fetch_optional(pool)
                            .await
                            .map_err(|_| Denial::ServerError)?;
                    viewer = flags.map(|(is_active, is_bot)| UserFlags { is_active, is_bot });
                    if viewer.is_some_and(|flags| flags.is_active && !flags.is_bot) {
                        // `managed_llm_profile` seam: CE has no usable
                        // desktop profile; a BYOK key reads as
                        // BYOK-unsupported, else LLM-config-missing
                        // (same transcription as `app_issues`).
                        let keyed = assistant_has_api_key(pool, viewer_id)
                            .await
                            .map_err(|_| Denial::ServerError)?;
                        llm_profile = LlmProfile {
                            available: false,
                            reason_code: if keyed {
                                ManagedRunnerReason::BYOK_UNSUPPORTED.to_owned()
                            } else {
                                ManagedRunnerReason::LLM_CONFIG_MISSING.to_owned()
                            },
                        };
                        if llm_profile.available {
                            enrolled_exists =
                                enrolled_runner(pool, viewer_id, project_id, workspace_id)
                                    .await
                                    .map_err(|_| Denial::ServerError)?;
                            if enrolled_exists {
                                online_exists =
                                    online_runner(pool, viewer_id, project_id, workspace_id)
                                        .await
                                        .map_err(|_| Denial::ServerError)?;
                                // `09738d222`: the paused
                                // tiebreaker runs only when
                                // nothing is online (mirrors
                                // `app_issues::handlers_core`).
                                if !online_exists {
                                    paused_exists =
                                        paused_runner(pool, viewer_id, project_id, workspace_id)
                                            .await
                                            .map_err(|_| Denial::ServerError)?;
                                }
                            }
                        }
                    }
                }
                let verdict = managed_runner_availability(
                    &settings.managed_runner,
                    viewer.as_ref(),
                    || llm_profile.clone(),
                    enrolled_exists,
                    online_exists,
                    paused_exists,
                );
                // `NO_RUNNER_FOR_PROJECT` accepts the pin (the desktop
                // enrolls on open and resolves the race, `:304-307`).
                if !verdict.available
                    && verdict.reason_code != ManagedRunnerReason::NO_RUNNER_FOR_PROJECT
                {
                    push_error(
                        errors,
                        "agent_executor",
                        managed_unavailable_detail(&verdict.reason_code).into_owned(),
                    );
                    return Ok(true);
                }
            }
            _ => {}
        }
    }
    // The mid-flight rule runs for explicit null too (`None` clears
    // the override); `None == None` is unchanged, like Django's
    // `str(None) == str(None)`.
    if executor.as_deref() != issue.agent_executor.as_deref()
        && has_active_run(pool, &issue.id).await?
    {
        push_error(
            errors,
            "agent_executor",
            "cannot change the execution target while the issue has an active run".to_owned(),
        );
        return Ok(true);
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Writes (`IssueCreateSerializer.update`, `IntakeIssueSerializer.update`,
// `Issue.save`, `BaseModel.save`)
// ---------------------------------------------------------------------------

/// `Issue.save` computed columns (`db/models/issue.py:300-348`):
/// `description_stripped` always recomputed; a null state resolves to
/// the project default (lowest sequence) or the lowest-sequence live
/// non-triage state **without touching `completed_at`** (the
/// group-driven arm is the `else` of the null-state arm); a set state
/// drives `completed_at` (now when the group is `completed`, else
/// null). `BaseModel.save` stamps `updated_by` on every update and
/// `auto_now` stamps `updated_at`.
pub struct IssueSaveOutcome {
    pub description_stripped: Option<String>,
    pub state_id: Option<Uuid>,
    pub completed_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

pub async fn issue_save_columns(
    pool: &PgPool,
    project_id: &Uuid,
    description_html: &str,
    state_id: Option<Uuid>,
    fallback_completed_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<IssueSaveOutcome, Denial> {
    // `Issue.save` strips with `utils/html_processor.py` (MLStripper,
    // entities decoded), not Django's verbatim `strip_tags`.
    let description_stripped = if description_html.is_empty() {
        None
    } else {
        Some(ml_strip_tags(description_html))
    };
    // The null-state arm assigns without touching `completed_at`.
    let state_was_null = state_id.is_none();
    let state_id = match state_id {
        None => default_state_for(pool, project_id).await?,
        Some(id) => Some(id),
    };
    let completed_at = if state_was_null {
        fallback_completed_at
    } else {
        match state_id {
            None => fallback_completed_at,
            Some(id) => {
                let group: Option<(Option<String>,)> =
                    sqlx::query_as(r#"SELECT "group" FROM states WHERE id = $1"#)
                        .bind(id)
                        .fetch_optional(pool)
                        .await
                        .map_err(|_| Denial::ServerError)?;
                match group {
                    Some((Some(group),)) if group == "completed" => Some(now),
                    _ => None,
                }
            }
        }
    };
    Ok(IssueSaveOutcome {
        description_stripped,
        state_id,
        completed_at,
        updated_at: now,
    })
}

/// Default-state resolution for a null state (`db/models/issue.py:300-314`,
/// `State.Meta.ordering = ("sequence",)`): the lowest-sequence live
/// default non-triage state, else the lowest-sequence live non-triage
/// state, else none.
pub async fn default_state_for(pool: &PgPool, project_id: &Uuid) -> Result<Option<Uuid>, Denial> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT s.id FROM states s
           WHERE s.project_id = $1 AND s.default AND s."group" != 'triage'
             AND s.deleted_at IS NULL
           ORDER BY s.sequence LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if row.is_some() {
        return Ok(row.map(|row| row.0));
    }
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT s.id FROM states s
           WHERE s.project_id = $1 AND s."group" != 'triage' AND s.deleted_at IS NULL
           ORDER BY s.sequence LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.0))
}

/// `IntakeIssueSerializer.validate` (`serializers/intake.py:43-66`):
/// accepting while the linked issue sits in TRIAGE needs a project
/// default state. Returns the default state id (for the transition)
/// or an error body.
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
pub async fn validate_accept_transition(
    pool: &PgPool,
    workspace_id: &Uuid,
    project_id: &Uuid,
    new_status: Option<i64>,
    issue_state_group: Option<String>,
) -> Result<Option<Uuid>, Response> {
    let accepted = new_status == Some(1);
    let triage = issue_state_group.as_deref() == Some("triage");
    if !accepted || !triage {
        return Ok(None);
    }
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT s.id FROM states s
           WHERE s.workspace_id = $1 AND s.project_id = $2 AND s.default
             AND s.deleted_at IS NULL LIMIT 1"#,
    )
    .bind(workspace_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    match row {
        Some((id,)) => Ok(Some(id)),
        // Live Django renders the dict-of-string detail list-wrapped
        // (`{"status": ["Cannot accept ..."]}`), verified against the
        // running backend — the services doc note claiming no list wrap
        // describes the helper, not the wire.
        None => Err(Denial::BadJson(serde_json::json!({
            "status": [pidash_services::app_intake::shape::NO_DEFAULT_STATE_MESSAGE],
        }))
        .into_response()),
    }
}

/// Apply the validated issue-branch writes (`update()`,
/// `serializers/issue.py:435-462`): m2m replacement, then the scalar
/// UPDATE with the `Issue.save` computed columns. Every save stamps
/// `updated_by` (`BaseModel.save`).
#[allow(clippy::too_many_arguments)]
pub async fn apply_issue_update(
    pool: &PgPool,
    issue: &IssueRow,
    validated: &ValidatedIssue,
    object: &IssueObjectValidation,
    actor_id: &Uuid,
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    if let Some(ids) = object.assignee_ids.as_ref() {
        // `update()` m2m replacement (`issue.py:474-475`): `IssueAssignee`
        // uses the soft-deletion manager, so the queryset `delete()` only
        // stamps `deleted_at` on the live rows (`db/mixins.py:48-53`) and
        // keeps every tombstone. A hard `DELETE` here would destroy history.
        sqlx::query(
            r#"UPDATE issue_assignees SET deleted_at = now() WHERE issue_id = $1 AND deleted_at IS NULL"#,
        )
        .bind(issue.id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        for assignee_id in ids {
            sqlx::query(
                r#"INSERT INTO issue_assignees
                   (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
                    project_id, workspace_id, issue_id, assignee_id)
                   VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, $9)
                   ON CONFLICT DO NOTHING"#,
            )
            .bind(Uuid::new_v4())
            .bind(now)
            .bind(now)
            .bind(issue.created_by_id)
            .bind(issue.updated_by_id)
            .bind(issue.project_id)
            .bind(issue.workspace_id)
            .bind(issue.id)
            .bind(assignee_id)
            .execute(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        }
    }
    if let Some(ids) = object.label_ids.as_ref() {
        // `update()` m2m replacement (`issue.py:495-496`): `IssueLabel`
        // uses the soft-deletion manager, so the queryset `delete()` only
        // stamps `deleted_at` on the live rows (`db/mixins.py:48-53`) and
        // keeps every tombstone. A hard `DELETE` here would destroy history.
        sqlx::query(
            r#"UPDATE issue_labels SET deleted_at = now() WHERE issue_id = $1 AND deleted_at IS NULL"#,
        )
        .bind(issue.id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        for label_id in ids {
            sqlx::query(
                r#"INSERT INTO issue_labels
                   (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
                    project_id, workspace_id, issue_id, label_id)
                   VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, $9)
                   ON CONFLICT DO NOTHING"#,
            )
            .bind(Uuid::new_v4())
            .bind(now)
            .bind(now)
            .bind(issue.created_by_id)
            .bind(issue.updated_by_id)
            .bind(issue.project_id)
            .bind(issue.workspace_id)
            .bind(issue.id)
            .bind(label_id)
            .execute(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        }
    }

    let html = object
        .description_html
        .clone()
        .or(validated.description_html.clone())
        .unwrap_or_else(|| issue.description_html.clone());
    let state_id = match validated.state_id.as_ref() {
        Some(inner) => *inner,
        None => issue.state_id,
    };
    let fallback_completed = validated.completed_at.unwrap_or(issue.completed_at);
    let saved = issue_save_columns(
        pool,
        &issue.project_id,
        &html,
        state_id,
        fallback_completed,
        now,
    )
    .await?;

    sqlx::query(
        r#"UPDATE issues SET name = $2, description_html = $3, description_json = $4,
             description_stripped = $5, priority = $6, complexity_score = $7,
             start_date = $8, target_date = $9, point = $10, sort_order = $11,
             completed_at = $12, archived_at = $13, is_draft = $14,
             external_source = $15, external_id = $16, sequence_id = $17,
             state_id = $18, parent_id = $19, estimate_point_id = $20, type_id = $21,
             assigned_pod_id = $22, agent_executor = $23, git_work_branch = $24,
             created_via = $25, updated_at = $26, updated_by_id = $27
           WHERE id = $1"#,
    )
    .bind(issue.id)
    .bind(validated.name.clone().unwrap_or_else(|| issue.name.clone()))
    .bind(html)
    .bind(
        validated
            .description_json
            .clone()
            .unwrap_or_else(|| issue.description_json.clone()),
    )
    .bind(saved.description_stripped)
    .bind(
        validated
            .priority
            .clone()
            .unwrap_or_else(|| issue.priority.clone()),
    )
    .bind(
        validated
            .complexity_score
            .map(|score| score as i32)
            .unwrap_or(issue.complexity_score),
    )
    .bind(validated.start_date.unwrap_or(issue.start_date))
    .bind(validated.target_date.unwrap_or(issue.target_date))
    .bind(
        validated
            .point
            .unwrap_or_else(|| issue.point.map(i64::from))
            .map(|point| point as i32),
    )
    .bind(validated.sort_order.unwrap_or(issue.sort_order))
    .bind(saved.completed_at)
    .bind(validated.archived_at.unwrap_or(issue.archived_at))
    .bind(validated.is_draft.unwrap_or(issue.is_draft))
    .bind(
        validated
            .external_source
            .clone()
            .unwrap_or_else(|| issue.external_source.clone()),
    )
    .bind(
        validated
            .external_id
            .clone()
            .unwrap_or_else(|| issue.external_id.clone()),
    )
    .bind(
        validated
            .sequence_id
            .map(|sequence| sequence as i32)
            .unwrap_or(issue.sequence_id),
    )
    .bind(saved.state_id)
    .bind(validated.parent_id.unwrap_or(issue.parent_id))
    .bind(
        validated
            .estimate_point_id
            .unwrap_or(issue.estimate_point_id),
    )
    .bind(validated.type_id.unwrap_or(issue.type_id))
    .bind(validated.assigned_pod_id.unwrap_or(issue.assigned_pod_id))
    .bind(
        validated
            .agent_executor
            .clone()
            .unwrap_or_else(|| issue.agent_executor.clone()),
    )
    .bind(
        validated
            .git_work_branch
            .clone()
            .unwrap_or_else(|| issue.git_work_branch.clone()),
    )
    .bind(
        validated
            .created_via
            .clone()
            .unwrap_or_else(|| issue.created_via.clone()),
    )
    .bind(saved.updated_at)
    .bind(actor_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Intake-issue field validation (`IntakeIssueSerializer`, partial)
// ---------------------------------------------------------------------------

/// Validated intake-branch updates.
#[derive(Debug, Default)]
pub struct ValidatedIntake {
    pub status: Option<i64>,
    pub duplicate_to: Option<Option<Uuid>>,
    pub snoozed_till: Option<Option<DateTime<Utc>>>,
    pub source: Option<Option<String>>,
}

const INTAKE_STATUSES: &[i64] = &[-2, -1, 0, 1, 2];

/// Field-level validation for the intake branch. Read-only keys (`id`,
/// `issue`, `created_by`, plus `project` / `workspace` which are not in
/// `Meta.fields` at all) are ignored, like DRF.
pub async fn validate_intake_fields(
    pool: &PgPool,
    input: &serde_json::Map<String, serde_json::Value>,
    errors: &mut FieldErrors,
) -> ValidatedIntake {
    let mut out = ValidatedIntake::default();
    if let Some(value) = input.get("status") {
        out.status = validate_int_choice(errors, "status", value, INTAKE_STATUSES, false);
    }
    if let Some(value) = input.get("duplicate_to") {
        if value.is_null() {
            out.duplicate_to = Some(None);
        } else if let Some(id) = validate_pk(
            pool,
            errors,
            "duplicate_to",
            value,
            duplicate_to_live_sql(),
            false,
        )
        .await
        {
            out.duplicate_to = Some(Some(id));
        }
    }
    if let Some(value) = input.get("snoozed_till") {
        if value.is_null() {
            out.snoozed_till = Some(None);
        } else {
            out.snoozed_till = validate_datetime(errors, "snoozed_till", value, false).map(Some);
        }
    }
    if let Some(value) = input.get("source") {
        if value.is_null() {
            out.source = Some(None);
        } else {
            out.source = validate_char(errors, "source", value, 255, true, false).map(Some);
        }
    }
    out
}

/// Apply the validated intake-branch writes (`update()`,
/// `serializers/intake.py:68-84`): scalar UPDATE with `updated_at` /
/// `updated_by` stamping, then the accepted-status TRIAGE-to-default
/// issue transition (a full `Issue.save`, so `updated_at` /
/// `description_stripped` / `completed_at` recompute there too).
///
/// `pre_save_issue` is the issue row fetched before either branch saved:
/// Django's transition saves the cached pre-save `intake_issue.issue`
/// whole, clobbering the issue-half writes from the same request (the
/// stale-save bug, PORTED BUGS PB-2036 — the same clobber the v1 port
/// models in `apply_triage_transition`).
pub async fn apply_intake_update(
    pool: &PgPool,
    bridge: &IntakeIssueRow,
    validated: &ValidatedIntake,
    pre_save_issue: &IssueRow,
    default_state_id: Option<Uuid>,
    actor_id: &Uuid,
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    sqlx::query(
        r#"UPDATE intake_issues SET status = $2, duplicate_to_id = $3, snoozed_till = $4,
             source = $5, updated_at = $6, updated_by_id = $7 WHERE id = $1"#,
    )
    .bind(bridge.id)
    .bind(
        validated
            .status
            .map(|status| status as i32)
            .unwrap_or(bridge.status),
    )
    .bind(validated.duplicate_to.unwrap_or(bridge.duplicate_to_id))
    .bind(validated.snoozed_till.unwrap_or(bridge.snoozed_till))
    .bind(
        validated
            .source
            .clone()
            .unwrap_or_else(|| bridge.source.clone()),
    )
    .bind(now)
    .bind(actor_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;

    // The accepted-status transition (`intake.py:73-82`): only when the
    // validated status is accepted (the caller checked TRIAGE + default
    // presence in `validate()`). The full-row rewrite IS the clobber:
    // every pre-save column is written back, so issue-half writes from
    // this request vanish — exactly Django's stale `issue.save()`.
    if let Some(state_id) = default_state_id {
        let transition_now = Utc::now();
        let saved = issue_save_columns(
            pool,
            &pre_save_issue.project_id,
            &pre_save_issue.description_html,
            Some(state_id),
            pre_save_issue.completed_at,
            transition_now,
        )
        .await?;
        sqlx::query(
            r#"UPDATE issues SET name = $2, description_html = $3, description_json = $4,
                 description_stripped = $5, priority = $6, complexity_score = $7,
                 start_date = $8, target_date = $9, point = $10, sort_order = $11,
                 completed_at = $12, archived_at = $13, is_draft = $14,
                 external_source = $15, external_id = $16, sequence_id = $17,
                 state_id = $18, parent_id = $19, estimate_point_id = $20, type_id = $21,
                 assigned_pod_id = $22, agent_executor = $23, git_work_branch = $24,
                 created_via = $25, updated_at = $26, updated_by_id = $27
               WHERE id = $1"#,
        )
        .bind(pre_save_issue.id)
        .bind(&pre_save_issue.name)
        .bind(&pre_save_issue.description_html)
        .bind(&pre_save_issue.description_json)
        .bind(saved.description_stripped)
        .bind(&pre_save_issue.priority)
        .bind(pre_save_issue.complexity_score)
        .bind(pre_save_issue.start_date)
        .bind(pre_save_issue.target_date)
        .bind(pre_save_issue.point)
        .bind(pre_save_issue.sort_order)
        .bind(saved.completed_at)
        .bind(pre_save_issue.archived_at)
        .bind(pre_save_issue.is_draft)
        .bind(&pre_save_issue.external_source)
        .bind(&pre_save_issue.external_id)
        .bind(pre_save_issue.sequence_id)
        .bind(state_id)
        .bind(pre_save_issue.parent_id)
        .bind(pre_save_issue.estimate_point_id)
        .bind(pre_save_issue.type_id)
        .bind(pre_save_issue.assigned_pod_id)
        .bind(&pre_save_issue.agent_executor)
        .bind(&pre_save_issue.git_work_branch)
        .bind(&pre_save_issue.created_via)
        .bind(saved.updated_at)
        .bind(actor_id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Rendering (`IntakeIssueDetailSerializer`, `IssueDetailSerializer`,
// `IssueIntakeSerializer`, `IntakeIssueSerializer`)
// ---------------------------------------------------------------------------

fn opt_uuid(value: &Option<Uuid>) -> serde_json::Value {
    value.map_or(serde_json::Value::Null, |id| {
        serde_json::Value::String(id.to_string())
    })
}

fn opt_string(value: &Option<String>) -> serde_json::Value {
    value
        .clone()
        .map_or(serde_json::Value::Null, serde_json::Value::String)
}

fn opt_date(value: &Option<chrono::NaiveDate>) -> serde_json::Value {
    value.map_or(serde_json::Value::Null, |date| {
        serde_json::Value::String(date.format("%Y-%m-%d").to_string())
    })
}

fn opt_dt(value: &Option<DateTime<Utc>>, timezone: chrono_tz::Tz) -> serde_json::Value {
    value.map_or(serde_json::Value::Null, |dt| {
        serde_json::Value::String(render_datetime_in(&dt, &timezone))
    })
}

/// Blocker summary (`orchestration/blockers.py:184-198`): live
/// `issue_relations` edges (self-edges excluded), open items first,
/// capped at 100, shaped as `{identifier, state, state_group}`.
/// A blocker edge row: issue id, sequence, project identifier, state id,
/// state name, state group.
type BlockerEdgeRow = (
    Uuid,
    i32,
    String,
    Option<Uuid>,
    Option<String>,
    Option<String>,
);

/// The ticker half of the agent blocks: the `issue_agent_ticker` row, the
/// project clock policy, and the issue's current state name/group for the
/// stage interval and the re-tick predicate. Rendered through the shared
/// `app_issues::serializers_detail` port, exactly like the app-issue
/// detail handler.
struct TickerOwned {
    ticker: IssueAgentTicker,
    policy: ProjectClockPolicy,
    state_group: Option<String>,
    state_name: Option<String>,
}

/// One `agent_run` row off the `LATEST/ACTIVE_AGENT_RUN_SQL` projection,
/// with pre-rendered `isoformat` datetimes (the services convention:
/// rows carry strings, `+00:00`, never the actor zone).
struct AgentRunOwned {
    id: String,
    status: String,
    executor_kind: String,
    queue_position: Option<i16>,
    runner_id: Option<String>,
    runner_name: Option<String>,
    created_at: String,
    assigned_at: Option<String>,
    started_at: Option<String>,
    ended_at: Option<String>,
    done_payload: Option<serde_json::Value>,
    error: String,
    error_code: String,
    llm_model: String,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    live: Option<AgentLiveOwned>,
}

/// The joined `runner_live_state` row, present exactly when the `LEFT
/// JOIN` hits (the `updated_at` column is non-null on a real row).
/// Carries the PIDASHCONV-1315 `agent_health`/`agent_last_activity_at`
/// columns the shared `LATEST/ACTIVE_AGENT_RUN_SQL` projection selects.
struct AgentLiveOwned {
    observed_run_id: Option<String>,
    last_event_at: Option<String>,
    last_event_kind: Option<String>,
    last_event_summary: Option<String>,
    agent_pid: Option<i32>,
    agent_subprocess_alive: Option<bool>,
    approvals_pending: Option<i32>,
    usage: serde_json::Value,
    llm_model: Option<String>,
    turn_count: Option<i32>,
    agent_health: Option<String>,
    agent_last_activity_at: Option<String>,
    updated_at: String,
}

pub struct BlockerSummary {
    pub blocked_by: Vec<serde_json::Value>,
    pub blocking: Vec<serde_json::Value>,
    pub has_open: bool,
}

pub async fn blocker_summary(
    pool: &PgPool,
    issue_id: &Uuid,
    workspace_id: &Uuid,
) -> Result<BlockerSummary, Denial> {
    // Forward `blocked_by` edges (blocker is `related_issue`) plus the
    // stored-reversed form (blocker is `issue`, type `blocking`).
    let blocked_ids: Vec<(Uuid,)> = sqlx::query_as(
        r#"SELECT DISTINCT related.issue_id FROM (
             SELECT r.related_issue_id AS issue_id FROM issue_relations r
             WHERE r.issue_id = $1 AND r.relation_type = 'blocked_by'
               AND r.deleted_at IS NULL AND r.related_issue_id != $1
               AND r.workspace_id = $2
             UNION
             SELECT r.issue_id AS issue_id FROM issue_relations r
             WHERE r.related_issue_id = $1 AND r.relation_type = 'blocking'
               AND r.deleted_at IS NULL AND r.issue_id != $1
               AND r.workspace_id = $2
           ) related"#,
    )
    .bind(issue_id)
    .bind(workspace_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let blocking_ids: Vec<(Uuid,)> = sqlx::query_as(
        r#"SELECT DISTINCT related.issue_id FROM (
             SELECT r.issue_id AS issue_id FROM issue_relations r
             WHERE r.related_issue_id = $1 AND r.relation_type = 'blocked_by'
               AND r.deleted_at IS NULL AND r.issue_id != $1
               AND r.workspace_id = $2
             UNION
             SELECT r.related_issue_id AS issue_id FROM issue_relations r
             WHERE r.issue_id = $1 AND r.relation_type = 'blocking'
               AND r.deleted_at IS NULL AND r.related_issue_id != $1
               AND r.workspace_id = $2
           ) related"#,
    )
    .bind(issue_id)
    .bind(workspace_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;

    async fn summarize(
        pool: &PgPool,
        ids: Vec<(Uuid,)>,
    ) -> Result<(Vec<serde_json::Value>, bool), Denial> {
        if ids.is_empty() {
            return Ok((Vec::new(), false));
        }
        let id_list: Vec<Uuid> = ids.into_iter().map(|row| row.0).collect();
        // `Issue.issue_objects`: live rows outside triage, unarchived,
        // undrafted, project unarchived.
        let rows: Vec<BlockerEdgeRow> = sqlx::query_as(
            r#"SELECT i.id, i.sequence_id, p.identifier, s.id, s.name, s."group"
               FROM issues i
               JOIN projects p ON p.id = i.project_id
               LEFT JOIN states s ON s.id = i.state_id
               LEFT JOIN states st ON st.id = i.state_id
               WHERE i.id = ANY($1) AND i.deleted_at IS NULL
                 AND (st."group" IS NULL OR st."group" != 'triage')
                 AND i.archived_at IS NULL AND NOT i.is_draft
                 AND p.deleted_at IS NULL AND p.archived_at IS NULL
               -- `_summary_list` (`blockers.py:172-181`): the `_resolved`
               -- annotation sorts open rows first and the cap applies
               -- after, so open items are never the ones cut. A NULL
               -- group takes the ELSE branch (open), like Django.
               ORDER BY CASE WHEN s."group" IN ('cancelled', 'completed')
                        THEN 1 ELSE 0 END,
                 p.identifier, i.sequence_id LIMIT 100"#,
        )
        .bind(&id_list)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        let mut items = Vec::with_capacity(rows.len());
        let mut has_open = false;
        // Open items first (`_resolved` 0 before 1), stable within each
        // band by the query order. The wire shape is exactly
        // `{identifier, state, state_group}`.
        for open_first in [true, false] {
            for (_id, sequence, identifier, _state_id, state_name, state_group) in &rows {
                let closed = matches!(state_group.as_deref(), Some("completed" | "cancelled"));
                if closed == open_first {
                    continue;
                }
                if open_first {
                    has_open = true;
                }
                items.push(serde_json::json!({
                    "identifier": format!("{identifier}-{sequence}"),
                    "state": state_name.clone(),
                    "state_group": state_group.clone(),
                }));
            }
        }
        Ok((items, has_open))
    }

    let (blocked_by, _) = summarize(pool, blocked_ids).await?;
    let (blocking, _) = summarize(pool, blocking_ids).await?;
    // `has_open_blockers`: any live open blocker, uncapped (the list
    // above caps at 100; the flag does not).
    let open_row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT b.id FROM issues b
           JOIN issue_relations r ON (
             (r.issue_id = $1 AND r.related_issue_id = b.id AND r.relation_type = 'blocked_by')
             OR (r.related_issue_id = $1 AND r.issue_id = b.id AND r.relation_type = 'blocking'))
           LEFT JOIN states s ON s.id = b.state_id
           JOIN projects p ON p.id = b.project_id
           WHERE r.deleted_at IS NULL AND r.issue_id != r.related_issue_id
             AND r.workspace_id = $2 AND b.workspace_id = $2
             AND b.deleted_at IS NULL
             AND (s."group" IS NULL OR s."group" NOT IN ('triage', 'completed', 'cancelled'))
             AND b.archived_at IS NULL AND NOT b.is_draft
             AND p.deleted_at IS NULL AND p.archived_at IS NULL
             AND (s."group" IS NULL OR s."group" NOT IN ('completed', 'cancelled'))
           LIMIT 1"#,
    )
    .bind(issue_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(BlockerSummary {
        blocked_by,
        blocking,
        has_open: open_row.is_some(),
    })
}

/// `get_agent_ticker` (`serializers/issue.py:1268-1318`): the reverse
/// `agent_ticker` relation, `None` when no row exists
/// (`ObjectDoesNotExist` → `None`). The row, project clock policy and
/// state feed the shared `agent_ticker_to_representation` port, so the
/// interval, pool math, re-tick predicate and `+00:00` datetimes match
/// the app-issue detail render exactly.
async fn fetch_ticker_owned(
    pool: &PgPool,
    issue: &IssueRow,
) -> Result<Option<TickerOwned>, Denial> {
    // Decoded positionally (20 columns exceed the `FromRow` tuple
    // arity); order matches the `SELECT` list below.
    let row = sqlx::query(
        r#"SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
                  issue_id, used, granted, waited, user_disabled, next_run_at, last_tick_at,
                  enabled, disarm_reason, pending_entry, pending_entry_free,
                  pending_entry_actor_id, pending_entry_trigger, resume_parent_run_id
           FROM issue_agent_ticker WHERE issue_id = $1 LIMIT 1"#,
    )
    .bind(issue.id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let id: Uuid = row.try_get(0).map_err(|_| Denial::ServerError)?;
    let created_at: DateTime<Utc> = row.try_get(1).map_err(|_| Denial::ServerError)?;
    let updated_at: DateTime<Utc> = row.try_get(2).map_err(|_| Denial::ServerError)?;
    let created_by_id: Option<Uuid> = row.try_get(3).map_err(|_| Denial::ServerError)?;
    let updated_by_id: Option<Uuid> = row.try_get(4).map_err(|_| Denial::ServerError)?;
    let deleted_at: Option<DateTime<Utc>> = row.try_get(5).map_err(|_| Denial::ServerError)?;
    let ticker_issue_id: Uuid = row.try_get(6).map_err(|_| Denial::ServerError)?;
    let used: i32 = row.try_get(7).map_err(|_| Denial::ServerError)?;
    let granted: i32 = row.try_get(8).map_err(|_| Denial::ServerError)?;
    let waited: i32 = row.try_get(9).map_err(|_| Denial::ServerError)?;
    let user_disabled: bool = row.try_get(10).map_err(|_| Denial::ServerError)?;
    let next_run_at: Option<DateTime<Utc>> = row.try_get(11).map_err(|_| Denial::ServerError)?;
    let last_tick_at: Option<DateTime<Utc>> = row.try_get(12).map_err(|_| Denial::ServerError)?;
    let enabled: bool = row.try_get(13).map_err(|_| Denial::ServerError)?;
    let disarm_reason: String = row.try_get(14).map_err(|_| Denial::ServerError)?;
    let pending_entry: bool = row.try_get(15).map_err(|_| Denial::ServerError)?;
    let pending_entry_free: bool = row.try_get(16).map_err(|_| Denial::ServerError)?;
    let pending_entry_actor_id: Option<Uuid> = row.try_get(17).map_err(|_| Denial::ServerError)?;
    let pending_entry_trigger: String = row.try_get(18).map_err(|_| Denial::ServerError)?;
    let resume_parent_run_id: Option<Uuid> = row.try_get(19).map_err(|_| Denial::ServerError)?;
    // Project clock policy (`db/models/project.py:157-160`): the pool and
    // the three per-stage interval columns, all non-null integers.
    let policy_row: Option<(i32, i32, i32, i32, bool)> = sqlx::query_as(
        r#"SELECT agent_default_max_ticks, agent_default_interval_seconds,
                  agent_review_default_interval_seconds, agent_test_default_interval_seconds,
                  agent_ticking_enabled
           FROM projects WHERE id = $1"#,
    )
    .bind(issue.project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (pool_size, impl_interval, review_interval, test_interval, ticking_enabled) =
        policy_row.unwrap_or((10, 10800, 10800, 10800, true));
    let state: Option<(String, String)> = match issue.state_id {
        None => None,
        Some(state_id) => sqlx::query_as(r#"SELECT "group", name FROM states WHERE id = $1"#)
            .bind(state_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?,
    };
    let (state_group, state_name) = match state {
        None => (None, None),
        Some((group, name)) => (Some(group), Some(name)),
    };
    Ok(Some(TickerOwned {
        ticker: IssueAgentTicker {
            id,
            created_at,
            updated_at,
            created_by_id,
            updated_by_id,
            deleted_at,
            issue_id: ticker_issue_id,
            used,
            granted,
            waited,
            user_disabled,
            next_run_at,
            last_tick_at,
            enabled,
            disarm_reason,
            pending_entry,
            pending_entry_free,
            pending_entry_actor_id,
            pending_entry_trigger,
            resume_parent_run_id,
        },
        policy: ProjectClockPolicy {
            agent_ticking_enabled: Some(ticking_enabled),
            agent_default_max_ticks: Some(pool_size),
            agent_default_interval_seconds: Some(i64::from(impl_interval)),
            agent_review_default_interval_seconds: Some(i64::from(review_interval)),
            agent_test_default_interval_seconds: Some(i64::from(test_interval)),
        },
        state_group,
        state_name,
    }))
}

/// One [`LATEST_AGENT_RUN_SQL`] / [`ACTIVE_AGENT_RUN_SQL`] row (the two
/// share their projection; only the `WHERE` differs), decoded
/// positionally — the projection selects `llm_model` twice, so name
/// lookup would bind the wrong one. Token counts come from the
/// `agent_run` generated columns (`usage->>'input'/'output'/'total'`);
/// live-state tokens ride the joined `usage` JSON for the shared
/// `flat_token_fields` port. Datetimes pre-render to `isoformat`
/// (`+00:00`), the services convention.
async fn fetch_agent_run(
    pool: &PgPool,
    sql: &str,
    issue_id: &Uuid,
) -> Result<Option<AgentRunOwned>, Denial> {
    let row = sqlx::query(sql)
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let iso = |moment: Option<DateTime<Utc>>| moment.map(serialize_iso_datetime);
    let id: Uuid = row.try_get(0).map_err(|_| Denial::ServerError)?;
    let status: String = row.try_get(1).map_err(|_| Denial::ServerError)?;
    let executor_kind: String = row.try_get(2).map_err(|_| Denial::ServerError)?;
    let queue_position: Option<i16> = row.try_get(3).map_err(|_| Denial::ServerError)?;
    let runner_id: Option<Uuid> = row.try_get(4).map_err(|_| Denial::ServerError)?;
    let created_at: DateTime<Utc> = row.try_get(5).map_err(|_| Denial::ServerError)?;
    let assigned_at: Option<DateTime<Utc>> = row.try_get(6).map_err(|_| Denial::ServerError)?;
    let started_at: Option<DateTime<Utc>> = row.try_get(7).map_err(|_| Denial::ServerError)?;
    let ended_at: Option<DateTime<Utc>> = row.try_get(8).map_err(|_| Denial::ServerError)?;
    let done_payload: Option<serde_json::Value> =
        row.try_get(9).map_err(|_| Denial::ServerError)?;
    let error: String = row.try_get(10).map_err(|_| Denial::ServerError)?;
    let error_code: String = row.try_get(11).map_err(|_| Denial::ServerError)?;
    let llm_model: String = row.try_get(12).map_err(|_| Denial::ServerError)?;
    let input_tokens: Option<i64> = row.try_get(13).map_err(|_| Denial::ServerError)?;
    let output_tokens: Option<i64> = row.try_get(14).map_err(|_| Denial::ServerError)?;
    let total_tokens: Option<i64> = row.try_get(15).map_err(|_| Denial::ServerError)?;
    let runner_name: Option<String> = row.try_get(16).map_err(|_| Denial::ServerError)?;
    let observed_run_id: Option<Uuid> = row.try_get(17).map_err(|_| Denial::ServerError)?;
    let last_event_at: Option<DateTime<Utc>> = row.try_get(18).map_err(|_| Denial::ServerError)?;
    let last_event_kind: Option<String> = row.try_get(19).map_err(|_| Denial::ServerError)?;
    let last_event_summary: Option<String> = row.try_get(20).map_err(|_| Denial::ServerError)?;
    let agent_pid: Option<i32> = row.try_get(21).map_err(|_| Denial::ServerError)?;
    let agent_subprocess_alive: Option<bool> = row.try_get(22).map_err(|_| Denial::ServerError)?;
    let approvals_pending: Option<i32> = row.try_get(23).map_err(|_| Denial::ServerError)?;
    let usage: Option<serde_json::Value> = row.try_get(24).map_err(|_| Denial::ServerError)?;
    let live_llm_model: Option<String> = row.try_get(25).map_err(|_| Denial::ServerError)?;
    let turn_count: Option<i32> = row.try_get(26).map_err(|_| Denial::ServerError)?;
    let agent_health: Option<String> = row.try_get(27).map_err(|_| Denial::ServerError)?;
    let agent_last_activity_at: Option<DateTime<Utc>> =
        row.try_get(28).map_err(|_| Denial::ServerError)?;
    let live_updated_at: Option<DateTime<Utc>> =
        row.try_get(29).map_err(|_| Denial::ServerError)?;
    let live = live_updated_at.map(|updated_at| AgentLiveOwned {
        observed_run_id: observed_run_id.map(|id| id.to_string()),
        last_event_at: iso(last_event_at),
        last_event_kind,
        last_event_summary,
        agent_pid,
        agent_subprocess_alive,
        approvals_pending,
        usage: usage.unwrap_or(serde_json::Value::Null),
        llm_model: live_llm_model,
        turn_count,
        agent_health,
        agent_last_activity_at: iso(agent_last_activity_at),
        updated_at: serialize_iso_datetime(updated_at),
    });
    Ok(Some(AgentRunOwned {
        id: id.to_string(),
        status,
        executor_kind,
        queue_position,
        runner_id: runner_id.map(|id| id.to_string()),
        runner_name,
        created_at: serialize_iso_datetime(created_at),
        assigned_at: iso(assigned_at),
        started_at: iso(started_at),
        ended_at: iso(ended_at),
        done_payload,
        error,
        error_code,
        llm_model,
        input_tokens,
        output_tokens,
        total_tokens,
        live,
    }))
}

/// The services [`AgentRunDetailRow`] view over an owned run row.
fn agent_run_detail_row(run: &AgentRunOwned) -> AgentRunDetailRow<'_> {
    let live_state = run.live.as_ref().map(|live| AgentLiveStateRow {
        observed_run_id: live.observed_run_id.as_deref(),
        last_event_at: live.last_event_at.as_deref(),
        last_event_kind: live.last_event_kind.as_deref(),
        last_event_summary: live.last_event_summary.as_deref(),
        agent_pid: live.agent_pid,
        agent_subprocess_alive: live.agent_subprocess_alive,
        approvals_pending: live.approvals_pending,
        usage: &live.usage,
        llm_model: live.llm_model.as_deref(),
        turn_count: live.turn_count,
        agent_health: live.agent_health.as_deref(),
        agent_last_activity_at: live.agent_last_activity_at.as_deref(),
        updated_at: live.updated_at.as_str(),
    });
    AgentRunDetailRow {
        id: run.id.as_str(),
        status: run.status.as_str(),
        executor_kind: run.executor_kind.as_str(),
        queue_position: run.queue_position,
        runner_id: run.runner_id.as_deref(),
        runner_name: run.runner_name.as_deref(),
        created_at: run.created_at.as_str(),
        assigned_at: run.assigned_at.as_deref(),
        started_at: run.started_at.as_deref(),
        ended_at: run.ended_at.as_deref(),
        done_payload: run.done_payload.as_ref().filter(|value| !value.is_null()),
        error: run.error.as_str(),
        error_code: run.error_code.as_str(),
        llm_model: run.llm_model.as_str(),
        input_tokens: run.input_tokens,
        output_tokens: run.output_tokens,
        total_tokens: run.total_tokens,
        live_state,
    }
}

/// `get_agent_status` (`serializers/issue.py:1373-1412`): the ticker
/// plus the latest and active runs. `None` when neither exists. Runs
/// and the count come from the shared agent-run statements; the block
/// renders through the shared `agent_status_to_representation` port,
/// so tokens, the live-state guard, `error_diagnostic` and the
/// `+00:00` datetimes match the app-issue detail render exactly.
pub async fn render_agent_status(
    pool: &PgPool,
    issue: &IssueRow,
    ticker: Option<AgentTickerView<'_>>,
) -> Result<serde_json::Value, Denial> {
    let latest = fetch_agent_run(pool, LATEST_AGENT_RUN_SQL, &issue.id).await?;
    let active = fetch_agent_run(pool, ACTIVE_AGENT_RUN_SQL, &issue.id).await?;
    let run_count: i64 = sqlx::query_scalar(AGENT_RUN_COUNT_SQL)
        .bind(issue.id)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let latest_row = latest.as_ref().map(agent_run_detail_row);
    let active_row = active.as_ref().map(agent_run_detail_row);
    let status =
        agent_status_to_representation(ticker, active_row.as_ref(), latest_row.as_ref(), run_count);
    match status {
        None => Ok(serde_json::Value::Null),
        Some(view) => serde_json::to_value(&view).map_err(|_| Denial::ServerError),
    }
}

/// `IssueIntakeSerializer` (`serializers/issue.py:1021-1036`): the 8
/// keys in declaration order. `label_ids` rides the list-query
/// annotation; a plain instance (no annotation) skips the key — the
/// same missing-attribute skip the live backend exhibits.
pub fn render_issue_intake(
    issue: &IssueRow,
    label_ids: Option<&[Uuid]>,
    timezone: chrono_tz::Tz,
) -> serde_json::Value {
    let mut map = serde_json::Map::with_capacity(8);
    map.insert(
        "id".to_owned(),
        serde_json::Value::String(issue.id.to_string()),
    );
    map.insert(
        "name".to_owned(),
        serde_json::Value::String(issue.name.clone()),
    );
    map.insert(
        "priority".to_owned(),
        serde_json::Value::String(issue.priority.clone()),
    );
    map.insert(
        "sequence_id".to_owned(),
        serde_json::Value::from(issue.sequence_id),
    );
    map.insert(
        "project_id".to_owned(),
        serde_json::Value::String(issue.project_id.to_string()),
    );
    map.insert(
        "created_at".to_owned(),
        serde_json::Value::String(render_datetime_in(&issue.created_at, &timezone)),
    );
    if let Some(ids) = label_ids {
        map.insert(
            "label_ids".to_owned(),
            ids.iter()
                .map(|id| serde_json::Value::String(id.to_string()))
                .collect(),
        );
    }
    map.insert("created_by".to_owned(), opt_uuid(&issue.created_by_id));
    serde_json::Value::Object(map)
}

/// `IssueDetailSerializer` (`serializers/issue.py:1034-1070,1226+`):
/// the 29 wire keys in declaration order. The seven annotation/
/// relation-backed keys the detail fetches never carry (`cycle_id`,
/// `module_ids`, `sub_issues_count`, `attachment_count`, `link_count`,
/// `is_subscribed`, `is_intake`) are absent, exactly like the live
/// backend renders them.
#[allow(clippy::too_many_arguments)]
pub async fn render_issue_detail(
    pool: &PgPool,
    issue: &IssueRow,
    label_ids: &[Uuid],
    assignee_ids: &[Uuid],
    timezone: chrono_tz::Tz,
) -> Result<serde_json::Value, Denial> {
    let synced = issue
        .external_source
        .as_deref()
        .is_some_and(|source| !source.is_empty())
        && is_synced_safe(pool, issue).await;
    // The agent blocks render through the shared app-issue ports
    // (`serializers/issue.py:1268-1412`); Django calls `get_agent_ticker`
    // once per block, so both blocks carry the same values.
    let ticker_owned = fetch_ticker_owned(pool, issue).await?;
    let ticker_view = ticker_owned.as_ref().map(|owned| {
        // `None` when stateless (or the state row is gone) — the impl
        // cadence, never ticking/paused.
        let state = match (owned.state_group.as_deref(), owned.state_name.as_deref()) {
            (Some(group), Some(name)) => Some(StateRef { group, name }),
            _ => None,
        };
        agent_ticker_to_representation(&owned.ticker, &owned.policy, state.as_ref())
    });
    let ticker = match &ticker_view {
        None => serde_json::Value::Null,
        Some(view) => serde_json::to_value(view).map_err(|_| Denial::ServerError)?,
    };
    let status = render_agent_status(pool, issue, ticker_view).await?;
    let summary = blocker_summary(pool, &issue.id, &issue.workspace_id).await?;
    let mut map = serde_json::Map::with_capacity(29);
    map.insert(
        "id".to_owned(),
        serde_json::Value::String(issue.id.to_string()),
    );
    map.insert(
        "name".to_owned(),
        serde_json::Value::String(issue.name.clone()),
    );
    map.insert("state_id".to_owned(), opt_uuid(&issue.state_id));
    map.insert(
        "sort_order".to_owned(),
        serde_json::Value::from(issue.sort_order),
    );
    map.insert(
        "completed_at".to_owned(),
        opt_dt(&issue.completed_at, timezone),
    );
    map.insert(
        "estimate_point".to_owned(),
        opt_uuid(&issue.estimate_point_id),
    );
    map.insert(
        "priority".to_owned(),
        serde_json::Value::String(issue.priority.clone()),
    );
    map.insert(
        "complexity_score".to_owned(),
        serde_json::Value::from(issue.complexity_score),
    );
    map.insert("start_date".to_owned(), opt_date(&issue.start_date));
    map.insert("target_date".to_owned(), opt_date(&issue.target_date));
    map.insert(
        "sequence_id".to_owned(),
        serde_json::Value::from(issue.sequence_id),
    );
    map.insert(
        "project_id".to_owned(),
        serde_json::Value::String(issue.project_id.to_string()),
    );
    map.insert("parent_id".to_owned(), opt_uuid(&issue.parent_id));
    map.insert(
        "assigned_pod_id".to_owned(),
        opt_uuid(&issue.assigned_pod_id),
    );
    map.insert(
        "agent_executor".to_owned(),
        opt_string(&issue.agent_executor),
    );
    map.insert(
        "label_ids".to_owned(),
        label_ids
            .iter()
            .map(|id| serde_json::Value::String(id.to_string()))
            .collect(),
    );
    map.insert(
        "assignee_ids".to_owned(),
        assignee_ids
            .iter()
            .map(|id| serde_json::Value::String(id.to_string()))
            .collect(),
    );
    map.insert(
        "created_at".to_owned(),
        serde_json::Value::String(render_datetime_in(&issue.created_at, &timezone)),
    );
    map.insert(
        "updated_at".to_owned(),
        serde_json::Value::String(render_datetime_in(&issue.updated_at, &timezone)),
    );
    map.insert("created_by".to_owned(), opt_uuid(&issue.created_by_id));
    map.insert("updated_by".to_owned(), opt_uuid(&issue.updated_by_id));
    map.insert(
        "is_draft".to_owned(),
        serde_json::Value::Bool(issue.is_draft),
    );
    map.insert("archived_at".to_owned(), opt_date(&issue.archived_at));
    map.insert("is_synced".to_owned(), serde_json::Value::Bool(synced));
    map.insert(
        "description_html".to_owned(),
        serde_json::Value::String(issue.description_html.clone()),
    );
    map.insert("agent_ticker".to_owned(), ticker);
    map.insert("agent_status".to_owned(), status);
    map.insert(
        "relations_summary".to_owned(),
        serde_json::json!({
            "blocked_by": summary.blocked_by,
            "blocking": summary.blocking,
        }),
    );
    map.insert(
        "has_open_blockers".to_owned(),
        serde_json::Value::Bool(summary.has_open),
    );
    Ok(serde_json::Value::Object(map))
}

/// `IntakeIssueDetailSerializer` shell (`serializers/intake.py:93-117`)
/// around an already-rendered nested issue.
pub fn render_detail_shell(
    bridge: &IntakeIssueRow,
    timezone: chrono_tz::Tz,
    duplicate_issue_detail: serde_json::Value,
    issue: serde_json::Value,
) -> serde_json::Value {
    let id = bridge.id.to_string();
    let duplicate_to = bridge.duplicate_to_id.map(|id| id.to_string());
    let snoozed_till = bridge
        .snoozed_till
        .as_ref()
        .map(|dt| render_datetime_in(dt, &timezone));
    let row = pidash_services::app_intake::shape::DetailRow {
        id: &id,
        status: i64::from(bridge.status),
        duplicate_to: duplicate_to.as_deref(),
        snoozed_till: snoozed_till.as_deref(),
        duplicate_issue_detail: Some(duplicate_issue_detail),
        source: bridge.source.as_deref(),
        issue,
    };
    pidash_services::app_intake::shape::render_detail(&row)
}

/// `IntakeIssueSerializer(...).data` for the intake-branch
/// `current_instance` dump (`:423`): the 7 write-serializer keys with
/// the nested `IssueIntakeSerializer` (no `label_ids` — the source
/// instance carries no annotation, so the key is skipped).
pub fn render_intake_dump(
    bridge: &IntakeIssueRow,
    timezone: chrono_tz::Tz,
    nested_issue: serde_json::Value,
) -> serde_json::Value {
    let mut map = serde_json::Map::with_capacity(7);
    map.insert(
        "id".to_owned(),
        serde_json::Value::String(bridge.id.to_string()),
    );
    map.insert("status".to_owned(), serde_json::Value::from(bridge.status));
    map.insert("duplicate_to".to_owned(), opt_uuid(&bridge.duplicate_to_id));
    map.insert(
        "snoozed_till".to_owned(),
        bridge
            .snoozed_till
            .as_ref()
            .map(|dt| serde_json::Value::String(render_datetime_in(dt, &timezone)))
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert("source".to_owned(), opt_string(&bridge.source));
    map.insert("issue".to_owned(), nested_issue);
    map.insert("created_by".to_owned(), opt_uuid(&bridge.created_by_id));
    serde_json::Value::Object(map)
}

/// `base_host(request, is_app=True)` (`utils/host.py:17`): `WEB_URL`
/// else `APP_BASE_URL`; unset is `ImproperlyConfigured` → 500.
fn request_origin(state: &AppState) -> Result<String, Denial> {
    state
        .settings()
        .urls
        .web_url
        .clone()
        .or_else(|| state.settings().urls.app_base_url.clone())
        .ok_or(Denial::ServerError)
}

/// Render the duplicate-issue nest for a detail response: the
/// `IssueIntakeSerializer` over the live `duplicate_to` row, or null
/// when the FK is null (or its row is gone).
async fn duplicate_detail(
    pool: &PgPool,
    duplicate_to: Option<Uuid>,
    timezone: chrono_tz::Tz,
) -> Result<serde_json::Value, Denial> {
    let Some(duplicate_id) = duplicate_to else {
        return Ok(serde_json::Value::Null);
    };
    let row = sqlx::query(r#"SELECT i.* FROM issues i WHERE i.id = $1 AND i.deleted_at IS NULL"#)
        .bind(duplicate_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    match row {
        None => Ok(serde_json::Value::Null),
        Some(row) => {
            let issue = IssueRow::get(&row).map_err(|_| Denial::ServerError)?;
            Ok(render_issue_intake(&issue, None, timezone))
        }
    }
}

/// `partial_update` (`base.py:329-500`).
pub async fn partial_update(
    State(state): State<AppState>,
    Path((slug, project_id_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    let pk = match parse_id(&pk_raw) {
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
    // The id rewrite only — no row lookup, so a denied caller never
    // learns whether the project exists (PIDASHCONV-1433).
    let project_id = match resolve_project_id(pool, &slug, &project_id_raw).await {
        Ok(project_id) => project_id,
        Err(denial) => return denial.into_response(),
    };
    // Decorator (`:328`): ADMIN + `Issue`-creator. Runs before the body
    // parse and every lookup, exactly like `@allow_permission`.
    let membership = match load_membership(pool, &slug, &project_id, &actor.id).await {
        Ok(membership) => membership,
        Err(denial) => return denial.into_response(),
    };
    let creator = match is_issue_creator(pool, &pk, &actor.id).await {
        Ok(creator) => creator,
        Err(denial) => return denial.into_response(),
    };
    let gate = guards::intake_issue_update_gate(&membership.as_guard(), creator);
    if gate.is_err() {
        return guard_denial(gate);
    }
    // DRF parses the body lazily at the first `request.data` touch
    // (`:330-331`), after the decorator.
    let data = match parse_body(&body) {
        Ok(data) => data,
        Err(response) => return response,
    };
    let serde_json::Value::Object(mut top) = data else {
        return Denial::ServerError.into_response();
    };

    // `skip_activity` pop + description-update probe (`:330-331`).
    let skip_activity = top
        .remove("skip_activity")
        .map(|value| json_truthy(&value))
        .unwrap_or(false);
    let is_description_update = top
        .get("description_html")
        .is_some_and(|value| !value.is_null());

    // Lookups (`:333-339`): the intake instance compiles to its pk, so
    // a missing intake reads as `IS NULL` → 404 (BUG-partial-…).
    let intake_id = match intake_id_for(pool, &slug, &project_id).await {
        Ok(intake_id) => intake_id,
        Err(denial) => return denial.into_response(),
    };
    let bridge = match lookup_intake_issue(pool, &pk, &slug, &project_id, intake_id).await {
        Ok(Some(bridge)) => bridge,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(denial) => return denial.into_response(),
    };
    // Membership gate (`:355-359`).
    let gate = guards::update_membership_gate(&membership.as_guard());
    if gate.is_err() {
        return guard_denial(gate);
    }
    // Low-role gate (`:362-368`): the bridge creator, not the issue
    // creator, decides here.
    let bridge_creator = bridge.created_by_id == Some(actor.id);
    let gate = guards::update_creator_gate(&membership.as_guard(), bridge_creator);
    if gate.is_err() {
        return guard_denial(gate);
    }

    // Issue branch (`:370-456`).
    let issue_value = top
        .remove("issue")
        .unwrap_or(serde_json::Value::Bool(false));
    let mut issue_validated: Option<ValidatedIssue> = None;
    let mut issue_object: Option<IssueObjectValidation> = None;
    let mut issue_requested_dump: Option<String> = None;
    let mut issue_current_dump: Option<String> = None;
    if json_truthy(&issue_value) {
        let serde_json::Value::Object(issue_input) = &issue_value else {
            return Denial::ServerError.into_response();
        };
        let stored = match fetch_issue_annotated(pool, &bridge.issue_id, &project_id, &slug).await {
            Ok(Some(stored)) => stored,
            Ok(None) => return Denial::NotFound.into_response(),
            Err(denial) => return denial.into_response(),
        };
        let (stored_issue, stored_labels, stored_assignees) = stored;
        // GUEST narrowing (`:398-403`): the 3-key subset with stored
        // fallbacks; every other key is silently dropped.
        let mut narrowed = serde_json::Map::new();
        if guards::guest_issue_narrowed(&membership.as_guard()) {
            for key in guards::GUEST_ISSUE_KEYS {
                let fallback = match key {
                    "name" => serde_json::Value::String(stored_issue.name.clone()),
                    "description_html" => {
                        serde_json::Value::String(stored_issue.description_html.clone())
                    }
                    _ => stored_issue.description_json.clone(),
                };
                narrowed.insert(
                    key.to_string(),
                    issue_input.get(key).cloned().unwrap_or(fallback),
                );
            }
        } else {
            narrowed = issue_input.clone();
        }
        issue_current_dump = match render_issue_detail(
            pool,
            &stored_issue,
            &stored_labels,
            &stored_assignees,
            actor.timezone,
        )
        .await
        {
            Ok(rendered) => Some(super::python_dumps(&rendered)),
            Err(denial) => return denial.into_response(),
        };
        issue_requested_dump = Some(super::python_dumps(&serde_json::Value::Object(
            narrowed.clone(),
        )));
        let mut field_errors = FieldErrors::new();
        let validated = validate_issue_fields(pool, &narrowed, &mut field_errors).await;
        if !field_errors.is_empty() {
            // Field errors skip `validate()` entirely (DRF order).
            return Denial::BadJson(serde_json::Value::Object(field_errors)).into_response();
        }
        let mut non_field: Vec<String> = Vec::new();
        let object = match validate_issue_object(
            pool,
            state.settings(),
            &project_id,
            &bridge.workspace_id,
            &actor.id,
            &stored_issue,
            &validated,
            &narrowed,
            &mut field_errors,
            &mut non_field,
        )
        .await
        {
            Ok(object) => object,
            Err(denial) => return denial.into_response(),
        };
        if !field_errors.is_empty() || !non_field.is_empty() {
            if !non_field.is_empty() {
                field_errors.insert(
                    "non_field_errors".to_owned(),
                    non_field
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                );
            }
            return Denial::BadJson(serde_json::Value::Object(field_errors)).into_response();
        }
        issue_validated = Some(validated);
        issue_object = Some(object);
    }

    // Intake branch (`:418-427`): only writers validate.
    let mut intake_validated: Option<ValidatedIntake> = None;
    let mut intake_current_dump: Option<String> = None;
    let mut accept_state: Option<Uuid> = None;
    // The pre-save issue snapshot for the accept transition's stale
    // full-row rewrite (`intake.py:70-82` saves the cached pre-save
    // `intake_issue.issue` whole). Fetched before either branch saves.
    let mut accept_presave: Option<IssueRow> = None;
    if guards::intake_fields_writable(&membership.as_guard()) {
        let bridge_issue =
            match fetch_issue_annotated(pool, &bridge.issue_id, &project_id, &slug).await {
                Ok(Some((issue, _, _))) => issue,
                Ok(None) => return Denial::NotFound.into_response(),
                Err(denial) => return denial.into_response(),
            };
        let nested = render_issue_intake(&bridge_issue, None, actor.timezone);
        intake_current_dump = Some(super::python_dumps(&render_intake_dump(
            &bridge,
            actor.timezone,
            nested,
        )));
        let mut field_errors = FieldErrors::new();
        let validated = validate_intake_fields(pool, &top, &mut field_errors).await;
        if !field_errors.is_empty() {
            return Denial::BadJson(serde_json::Value::Object(field_errors)).into_response();
        }
        // `validate()`: the accepted-status default-state check
        // (`intake.py:43-66`).
        let state_group: Option<String> = match bridge_issue.state_id {
            None => None,
            Some(state_id) => {
                let row: Option<(Option<String>,)> =
                    match sqlx::query_as(r#"SELECT "group" FROM states WHERE id = $1"#)
                        .bind(state_id)
                        .fetch_optional(pool)
                        .await
                    {
                        Ok(row) => row,
                        Err(_) => return Denial::ServerError.into_response(),
                    };
                row.and_then(|row| row.0)
            }
        };
        match validate_accept_transition(
            pool,
            &bridge.workspace_id,
            &project_id,
            validated.status,
            state_group,
        )
        .await
        {
            Ok(default_state) => accept_state = default_state,
            Err(response) => return response,
        }
        intake_validated = Some(validated);
        accept_presave = Some(bridge_issue);
    }

    // Both serializers valid: save the issue branch first (`:430-456`).
    let now = Utc::now();
    let origin = match request_origin(&state) {
        Ok(origin) => origin,
        Err(denial) => return denial.into_response(),
    };
    if let (Some(validated), Some(object), Some(requested), Some(current)) = (
        issue_validated.as_ref(),
        issue_object.as_ref(),
        issue_requested_dump.as_ref(),
        issue_current_dump.as_ref(),
    ) {
        let stored = match fetch_issue_annotated(pool, &bridge.issue_id, &project_id, &slug).await {
            Ok(Some((issue, _, _))) => issue,
            Ok(None) => return Denial::NotFound.into_response(),
            Err(denial) => return denial.into_response(),
        };
        if let Err(denial) =
            apply_issue_update(pool, &stored, validated, object, &actor.id, now).await
        {
            return denial.into_response();
        }
        // The migration-update silent path (`:434`): no emits at all.
        let migration =
            intake_tasks::is_migration_description_update(skip_activity, is_description_update);
        if !migration {
            let epoch = Utc::now().timestamp();
            let activity = intake_tasks::intake_update_issue_activity(
                requested.clone(),
                actor.id.to_string(),
                pk.to_string(),
                project_id.to_string(),
                current.clone(),
                epoch,
                origin.clone(),
                bridge.id.to_string(),
            );
            enqueue_message(
                pool,
                pidash_jobs::TaskMessage::new(activity.task_name(), vec![], activity.kwargs()),
            )
            .await;
            let version = intake_tasks::intake_update_description_version(
                current.clone(),
                pk.to_string(),
                actor.id.to_string(),
            );
            enqueue_message(
                pool,
                pidash_jobs::TaskMessage::new(version.task_name(), vec![], version.kwargs()),
            )
            .await;
        }
    }

    // Then the intake branch (`:457-471`).
    if let (Some(validated), Some(current), Some(presave)) = (
        intake_validated.as_ref(),
        intake_current_dump.as_ref(),
        accept_presave.as_ref(),
    ) {
        if let Err(denial) = apply_intake_update(
            pool,
            &bridge,
            validated,
            presave,
            accept_state,
            &actor.id,
            now,
        )
        .await
        {
            return denial.into_response();
        }
        let epoch = Utc::now().timestamp();
        let activity = intake_tasks::intake_update_intake_activity(
            super::python_dumps(&serde_json::Value::Object(top.clone())),
            actor.id.to_string(),
            pk.to_string(),
            project_id.to_string(),
            current.clone(),
            epoch,
            origin,
            bridge.id.to_string(),
        );
        enqueue_message(
            pool,
            pidash_jobs::TaskMessage::new(activity.task_name(), vec![], activity.kwargs()),
        )
        .await;
    }

    // Re-fetch and render (`:473-500`).
    let Some(intake_uuid) = intake_id else {
        return Denial::ServerError.into_response();
    };
    let detail = match fetch_detail(pool, &intake_uuid, &pk, &project_id).await {
        Ok(Some(detail)) => detail,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(denial) => return denial.into_response(),
    };
    let (bridge, issue, label_ids, assignee_ids) = detail;
    let nested =
        match render_issue_detail(pool, &issue, &label_ids, &assignee_ids, actor.timezone).await {
            Ok(nested) => nested,
            Err(denial) => return denial.into_response(),
        };
    let duplicate = match duplicate_detail(pool, bridge.duplicate_to_id, actor.timezone).await {
        Ok(duplicate) => duplicate,
        Err(denial) => return denial.into_response(),
    };
    raw_json_response(render_detail_shell(&bridge, actor.timezone, duplicate, nested).to_string())
}

/// `retrieve` (`base.py:503-547`).
pub async fn retrieve(
    State(state): State<AppState>,
    Path((slug, project_id_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pk = match parse_id(&pk_raw) {
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
    // The id rewrite only — no row lookup, so a denied caller never
    // learns whether the project exists (PIDASHCONV-1433).
    let project_id = match resolve_project_id(pool, &slug, &project_id_raw).await {
        Ok(project_id) => project_id,
        Err(denial) => return denial.into_response(),
    };
    // Decorator (`:502`): ADMIN + MEMBER + GUEST + `Issue`-creator.
    // Runs before every lookup, exactly like `@allow_permission`.
    let creator = match is_issue_creator(pool, &pk, &actor.id).await {
        Ok(creator) => creator,
        Err(denial) => return denial.into_response(),
    };
    let membership = match load_membership(pool, &slug, &project_id, &actor.id).await {
        Ok(membership) => membership,
        Err(denial) => return denial.into_response(),
    };
    let gate = guards::intake_issue_retrieve_gate(&membership.as_guard(), creator);
    if gate.is_err() {
        return guard_denial(gate);
    }
    // `Intake.objects.filter(...).first()` then `.get(intake_id=intake_id.id)`
    // — a missing intake dereferences `None.id` → 500.
    let intake_id = match intake_id_for(pool, &slug, &project_id).await {
        Ok(Some(intake_id)) => intake_id,
        Ok(None) => return Denial::ServerError.into_response(),
        Err(denial) => return denial.into_response(),
    };
    // `Project.objects.get(pk=project_id)` — a miss is 404.
    let view_all = match guest_view_all(pool, &project_id).await {
        Ok(view_all) => view_all,
        Err(denial) => return denial.into_response(),
    };
    let detail = match fetch_detail(pool, &intake_id, &pk, &project_id).await {
        Ok(Some(detail)) => detail,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(denial) => return denial.into_response(),
    };
    let (bridge, issue, label_ids, assignee_ids) = detail;
    // Guest creator check (`:531-545`): the *bridge* creator decides.
    let bridge_creator = bridge.created_by_id == Some(actor.id);
    let gate = guards::guest_view_gate(&membership.as_guard(), view_all, bridge_creator);
    if gate.is_err() {
        return guard_denial(gate);
    }
    let nested =
        match render_issue_detail(pool, &issue, &label_ids, &assignee_ids, actor.timezone).await {
            Ok(nested) => nested,
            Err(denial) => return denial.into_response(),
        };
    let duplicate = match duplicate_detail(pool, bridge.duplicate_to_id, actor.timezone).await {
        Ok(duplicate) => duplicate,
        Err(denial) => return denial.into_response(),
    };
    raw_json_response(render_detail_shell(&bridge, actor.timezone, duplicate, nested).to_string())
}

/// `destroy` (`base.py:550-566`).
pub async fn destroy(
    State(state): State<AppState>,
    Path((slug, project_id_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pk = match parse_id(&pk_raw) {
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
    // The id rewrite only — no row lookup, so a denied caller never
    // learns whether the project exists (PIDASHCONV-1433).
    let project_id = match resolve_project_id(pool, &slug, &project_id_raw).await {
        Ok(project_id) => project_id,
        Err(denial) => return denial.into_response(),
    };
    // Decorator (`:549`): ADMIN + `Issue`-creator. Runs before every
    // lookup, exactly like `@allow_permission`.
    let creator = match is_issue_creator(pool, &pk, &actor.id).await {
        Ok(creator) => creator,
        Err(denial) => return denial.into_response(),
    };
    let membership = match load_membership(pool, &slug, &project_id, &actor.id).await {
        Ok(membership) => membership,
        Err(denial) => return denial.into_response(),
    };
    let gate = guards::intake_issue_destroy_gate(&membership.as_guard(), creator);
    if gate.is_err() {
        return guard_denial(gate);
    }
    let intake_id = match intake_id_for(pool, &slug, &project_id).await {
        Ok(intake_id) => intake_id,
        Err(denial) => return denial.into_response(),
    };
    let bridge = match lookup_intake_issue(pool, &pk, &slug, &project_id, intake_id).await {
        Ok(Some(bridge)) => bridge,
        Ok(None) => return Denial::NotFound.into_response(),
        Err(denial) => return denial.into_response(),
    };

    // The status cascade (`:560-565`): statuses -2/-1/0/2 also delete
    // the `Issue`. Soft deletes stamp `deleted_at`/`updated_at`/
    // `updated_by` (`SoftDeleteModel.delete` + `BaseModel.save`) and
    // enqueue the related-objects sweep.
    let now = Utc::now();
    if guards::destroy_cascades_to_issue(bridge.status) {
        let issue = match fetch_issue_for_destroy(pool, &slug, &project_id, &pk).await {
            Ok(Some(issue)) => issue,
            // BUG-destroy-dangling-issue: `.first()` may be `None` and
            // `.delete()` is unconditional → 500.
            Ok(None) => return Denial::ServerError.into_response(),
            Err(denial) => return denial.into_response(),
        };
        if let Err(denial) = soft_delete_issue(pool, &issue.id, &actor.id, now).await {
            return denial.into_response();
        }
        enqueue_soft_delete(pool, "db", "issue", &pk).await;
    }
    if let Err(denial) = soft_delete_bridge(pool, &bridge.id, &actor.id, now).await {
        return denial.into_response();
    }
    enqueue_soft_delete(pool, "db", "intakeissue", &bridge.id).await;
    Response::builder()
        .status(axum::http::StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty 204")
}

/// `SoftDeleteModel.delete` (`db/mixins.py:71-77`): stamp + save.
async fn soft_delete_issue(
    pool: &PgPool,
    issue_id: &Uuid,
    actor_id: &Uuid,
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    sqlx::query(
        r#"UPDATE issues SET deleted_at = $2, updated_at = $2, updated_by_id = $3 WHERE id = $1"#,
    )
    .bind(issue_id)
    .bind(now)
    .bind(actor_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

async fn soft_delete_bridge(
    pool: &PgPool,
    bridge_id: &Uuid,
    actor_id: &Uuid,
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    sqlx::query(
        r#"UPDATE intake_issues SET deleted_at = $2, updated_at = $2, updated_by_id = $3 WHERE id = $1"#,
    )
    .bind(bridge_id)
    .bind(now)
    .bind(actor_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

// Intake-issue list + create (D-32, stage 5, PIDASHCONV-385).
//
// Port of `IntakeIssueViewSet.list` (`base.py:176-219`) and `.create`
// (`base.py:222-326`) from `apps/api/pi_dash/app/views/intake/base.py`,
// with collection routes from `app/urls/intake.py:29-56` (the
// `inbox-issues` alias shares the viewset).
//
// Action map:
//
// * `list`: intake-`None` 404 `{"error": "Intake not found"}`;
//   `issue_filters(GET, prefix "issue__")` compiled by [`compile_filter`];
//   `label_ids` annotate; `order_by` default `-issue__created_at`;
//   status CSV default `"-2"` minus `"null"` tokens; guest narrowing;
//   `BasePaginator` 12-key envelope with `IntakeIssueSerializer`
//   (`many=True`) rows. 200.
// * `create`: 400 `"Name is required"` unless `issue.name` is truthy;
//   400 `"Invalid priority"` unless the priority (default `"none"`) is in
//   the allowlist; triage state get-or-create; `IssueCreateSerializer`
//   with `allow_triage_state` context; `IntakeIssue` create with source
//   `IN_APP`; `issue.activity.created` + description-version emits;
//   re-fetch with label/assignee annotates; `IntakeIssueDetailSerializer`
//   200; serializer errors 400.
//
// Layering: param parsing, role gates and shapes live in
// `pidash_services::app_intake` and `pidash_db::app_intake`; the session
// gate (`@allow_permission([ADMIN, MEMBER, GUEST])` + `IsAuthenticated`
// order) is `crate::app_issues::resolve_gate`; the paginator envelope is
// `pidash_services::app_issues::envelope`. This module owns the HTTP
// shell, the `issue_filters` SQL compiler (transcribed from
// `api/src/app_issues` `legacy_sql`/`legacy_text_sql` against the real
// table names — those fns are private, so the compiler lives here
// instead of crossing the module boundary), the write path, and the row
// fetching.
//
// # Ported bugs and quirks (translate, don't redesign)
//
// * QUIRK-create-no-intake (`:265`): create looks the intake up with
//   `.first()` and dereferences `.id` unguarded, so a missing intake row
//   is a 500, not the list's 404. Ported as-is.
// * QUIRK-order-by-rejection (`:198`): an unknown `order_by` field is
//   Django's `FieldError` path (PIDASHCONV-1426) — rejected before SQL
//   rendering, answering the 500 envelope; user text never reaches the
//   statement. Ported as-is.
// * QUIRK-status-coercion (`:200-202`): a non-numeric status token fails
//   `IntegerField` coercion at execution (500); an explicit `?status=`
//   with only `"null"` tokens leaves no filter (all statuses list).
//   Ported as-is via `parse_intake_status`.
// * QUIRK-non-dict-issue (create `:223`): a non-object `issue` value
//   raises `AttributeError` before the name gate (500); explicit `null`
//   does the same. Ported as-is.
// * QUIRK-priority-model-default: a missing priority validates as
//   `"none"` and the serializer inserts the model default `"none"`
//   (unlike the space twin's `"low"` fallback — same serializer, the
//   space view passes the value explicitly).
// * QUIRK-per-page-zero: `?per_page=0` divides by zero in `max_hits`
//   (500); negative values take the same 500 arm here (Django answers
//   200 with degenerate slices — absurd input, divergence documented).
// * Fresh-create detail constants: a just-created issue has no ticker,
//   runs, relations or subscriptions, so `agent_ticker`/`agent_status`
//   are null, `relations_summary` is empty, `has_open_blockers` is
//   false, and `is_subscribed`/`is_intake` (declared read-only fields
//   with no model attribute) are skipped by DRF, not nulled. Likewise
//   `cycle_id`/`module_ids`/the three counts render only when the
//   instance carries the annotation, which the create re-fetch does not
//   select — absent keys, not nulls.
//
// Task delivery: `serve` carries no AMQP publisher (only the worker
// does), so `.delay()` calls enqueue a [`pidash_jobs::queue::NewJob`]
// into `rust_job_queue`; the worker forwards Python-owned names to the
// broker. Enqueue is best-effort after commit: a missing queue table
// must not turn user-visible writes into 500s, so failures are traced
// and the response stands.

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

/// `list` (`base.py:177-219`).
pub async fn collection_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    let gate = match resolve_gate(&state, &slug, &project_raw, extension).await {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    // `Intake.objects.filter(workspace__slug, project_id).first()`
    // (`:178`); default ordering is `name` (`db/models/intake.py:35`).
    // A miss answers 404 (`:179-180`).
    let intake_id: Option<Uuid> = match sqlx::query_scalar(
        "SELECT \"intakes\".\"id\" FROM \"intakes\" WHERE (\"intakes\".\"workspace_id\" = $1 AND \"intakes\".\"project_id\" = $2 AND \"intakes\".\"deleted_at\" IS NULL) ORDER BY \"intakes\".\"name\" ASC LIMIT 1",
    )
    .bind(gate.workspace_id)
    .bind(gate.project_id)
    .fetch_optional(pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some(intake_id) = intake_id else {
        return guards_error(guards::intake_not_found());
    };
    // `Project.objects.get(pk=...)` (`:182`) is subsumed by the gate's
    // tenant lookup (same 404 when the row is gone).
    let today = Utc::now().with_timezone(&gate.timezone).date_naive();
    let compiled = match compile_filter(&query, today) {
        Ok(compiled) => compiled,
        Err(response) => return response,
    };
    let order_by =
        query_last(&query, "order_by").unwrap_or_else(|| "-issue__created_at".to_string());
    let statuses = match queries::parse_intake_status(query_last(&query, "status").as_deref()) {
        Ok(statuses) => statuses,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let page = match list_page(&query) {
        Ok(page) => page,
        Err(response) => return response,
    };
    let list_query = queries::IntakeIssueListQuery {
        order_by: &order_by,
        statuses,
        issue_filters_sql: compiled.fragment.as_deref(),
        issue_filter_binds: compiled.bind_count,
        joins: &compiled.joins,
        guest_created_by: gate.guest_scoped,
        limit: page.limit,
        offset: page.offset,
    };
    // Unknown `order_by` is Django's `FieldError` path (`base.py:198`):
    // the generic 500, with nothing rendered into SQL (PIDASHCONV-1426).
    let Some(sql) = queries::intake_issue_list_sql(&list_query) else {
        return Denial::ServerError.into_response();
    };
    let mut statement = sqlx::query(&sql).bind(intake_id).bind(gate.project_id);
    statement = bind_owned(statement, &compiled.binds);
    if let Some(statuses) = &list_query.statuses {
        for status in statuses {
            statement = statement.bind(*status);
        }
    }
    if list_query.guest_created_by {
        statement = statement.bind(gate.user_id);
    }
    let rows = match statement.fetch_all(pool).await {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    // Total over the same scope (`queryset.count()`; the `GROUP BY`
    // collapses fanout, so `COUNT(DISTINCT id)` matches).
    let total: i64 = {
        let mut count_sql = String::from(
            "SELECT COUNT(DISTINCT \"intake_issues\".\"id\") FROM \"intake_issues\" LEFT OUTER JOIN \"issues\" ON (\"intake_issues\".\"issue_id\" = \"issues\".\"id\") LEFT OUTER JOIN \"issue_labels\" ON (\"issues\".\"id\" = \"issue_labels\".\"issue_id\") LEFT OUTER JOIN \"labels\" ON (\"issue_labels\".\"label_id\" = \"labels\".\"id\")",
        );
        count_sql.push_str(&compiled.joins);
        count_sql.push_str(" WHERE (\"intake_issues\".\"intake_id\" = $1 AND \"intake_issues\".\"project_id\" = $2 AND \"intake_issues\".\"deleted_at\" IS NULL");
        let mut next = 3;
        if let Some(fragment) = &compiled.fragment {
            count_sql.push_str(&format!(" AND ({fragment})"));
            next += compiled.bind_count;
        }
        if let Some(statuses) = &list_query.statuses {
            let holders = statuses
                .iter()
                .enumerate()
                .map(|(ix, _)| format!("${}", next + ix))
                .collect::<Vec<_>>()
                .join(", ");
            count_sql.push_str(&format!(" AND \"intake_issues\".\"status\" IN ({holders})"));
            next += statuses.len();
        }
        if list_query.guest_created_by {
            count_sql.push_str(&format!(
                " AND \"intake_issues\".\"created_by_id\" = ${next}"
            ));
        }
        count_sql.push(')');
        let mut count_statement = sqlx::query_scalar::<_, i64>(&count_sql)
            .bind(intake_id)
            .bind(gate.project_id);
        count_statement = bind_owned_scalar(count_statement, &compiled.binds);
        if let Some(statuses) = &list_query.statuses {
            for status in statuses {
                count_statement = count_statement.bind(*status);
            }
        }
        if list_query.guest_created_by {
            count_statement = count_statement.bind(gate.user_id);
        }
        match count_statement.fetch_one(pool).await {
            Ok(total) => total,
            Err(_) => return Denial::ServerError.into_response(),
        }
    };
    // The second fetch Django's `select_related("issue")` folds in: the
    // list projection selects only intake columns, so issue columns come
    // from one `IN` query (no soft-delete guard — `select_related`
    // follows the FK unconditionally).
    let issue_ids: Vec<Uuid> = rows
        .iter()
        .filter_map(|row| row.try_get::<Uuid, _>("issue_id").ok())
        .collect();
    let nested = match fetch_list_issues(pool, &issue_ids).await {
        Ok(nested) => nested,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mut rendered = Vec::with_capacity(rows.len());
    for row in &rows {
        let issue_id: Uuid = match row.try_get("issue_id") {
            Ok(id) => id,
            Err(_) => return Denial::ServerError.into_response(),
        };
        let Some(issue) = nested.get(&issue_id) else {
            return Denial::ServerError.into_response();
        };
        match render_list_row(row, issue, gate.timezone) {
            Ok(value) => rendered.push(value),
            Err(_) => return Denial::ServerError.into_response(),
        }
    }
    let results_json = serde_json::Value::Array(rendered).to_string();
    // `limit <= 0` never reaches here (`list_page` rejects it the way
    // the `ZeroDivisionError` in `max_hits` rejects `per_page=0`).
    // (`div_ceil` is unstable on this toolchain; the manual form matches
    // `math.ceil` for non-negative inputs.)
    let total_pages = (total + page.limit - 1) / page.limit;
    let next_cursor = format!("{}:{}:0", page.limit, page.page + 1);
    let prev_cursor = format!("{}:{}:1", page.limit, page.page - 1);
    // `next.has_results` is `page_results.count() > limit` over the
    // over-fetched window — exactly `total > offset + limit`.
    let body = pidash_services::app_issues::envelope(
        None,
        None,
        total,
        &next_cursor,
        &prev_cursor,
        total > page.offset + page.limit,
        page.page > 0,
        rows.len(),
        total_pages,
        total,
        &results_json,
    );
    raw_json_response(body)
}

// ---------------------------------------------------------------------------
// Pagination: BasePaginator.get_per_page + OffsetPaginator.get_result
// ---------------------------------------------------------------------------

/// The paginator window: `limit` (`per_page`), `page` (`cursor.offset`),
/// `offset` (`page * limit`).
struct ListPage {
    limit: i64,
    page: i64,
    offset: i64,
}

/// `BasePaginator.get_per_page` (`utils/paginator.py:642-652`) plus the
/// cursor half of `OffsetPaginator.get_result` (`:124-164`): the intake
/// list passes no `order_by` to the paginator, so no re-ordering applies
/// here. Errors answer DRF `ParseError` bodies (`{"detail": ...}` 400);
/// a negative page answers `{"detail": "Error in parsing"}` 400 via
/// `BadPaginationError`; `per_page <= 0` answers 500 (Django divides by
/// zero computing `max_hits`).
#[allow(clippy::result_large_err)]
fn list_page(query: &QueryMap) -> Result<ListPage, Response> {
    let per_page = match query_last(query, "per_page") {
        None => 1000,
        Some(raw) => match parse_python_int(&raw) {
            Some(value) => value,
            None => {
                return Err(
                    Denial::BadDetail("Invalid per_page parameter.".to_owned()).into_response()
                );
            }
        },
    };
    if per_page > 1000 {
        return Err(
            Denial::BadDetail("Invalid per_page value. Cannot exceed 1000.".to_owned())
                .into_response(),
        );
    }
    if per_page <= 0 {
        return Err(Denial::ServerError.into_response());
    }
    let raw_cursor = query_last(query, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let (_value, page, _is_prev) = match parse_cursor(&raw_cursor) {
        Some(cursor) => cursor,
        None => {
            return Err(Denial::BadDetail("Invalid cursor parameter.".to_owned()).into_response());
        }
    };
    let offset = page.saturating_mul(per_page);
    if offset < 0 {
        return Err(Denial::BadDetail("Error in parsing".to_owned()).into_response());
    }
    Ok(ListPage {
        limit: per_page,
        page,
        offset,
    })
}

/// Python `int()`: surrounding whitespace stripped, optional sign, ASCII
/// digits only (no float shapes, no underscores).
fn parse_python_int(raw: &str) -> Option<i64> {
    let text = raw.trim();
    let digits = text
        .strip_prefix('+')
        .or_else(|| text.strip_prefix('-'))
        .unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<i64>().ok()
}

/// `Cursor.from_string` (`utils/paginator.py:48-58`): exactly three
/// `:`-separated parts; float iff the value part contains `.`.
fn parse_cursor(raw: &str) -> Option<(CursorValue, i64, bool)> {
    let mut bits = raw.split(':');
    let (value_raw, offset_raw, prev_raw) =
        match (bits.next(), bits.next(), bits.next(), bits.next()) {
            (Some(v), Some(o), Some(p), None) => (v, o, p),
            _ => return None,
        };
    let value = if value_raw.contains('.') {
        CursorValue::Float(value_raw.parse::<f64>().ok()?)
    } else {
        CursorValue::Int(value_raw.parse::<i64>().ok()?)
    };
    let offset = offset_raw.parse::<i64>().ok()?;
    let is_prev = prev_raw.parse::<i64>().ok().map(|n| n != 0)?;
    Some((value, offset, is_prev))
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum CursorValue {
    Int(i64),
    Float(f64),
}

// ---------------------------------------------------------------------------
// issue_filters compiler: issue_filters(GET, prefix "issue__") to SQL
// ---------------------------------------------------------------------------

/// One dynamic bind, in placeholder order.
#[derive(Debug, Clone)]
enum OwnedBind {
    Uuid(Uuid),
    Text(String),
    Int(i32),
    Date(chrono::NaiveDate),
}

/// The compiled filter: extra JOINs, the `AND`-joined fragment (no
/// leading `AND`), and the binds in `$3..` order.
struct CompiledFilter {
    joins: String,
    fragment: Option<String>,
    binds: Vec<OwnedBind>,
    bind_count: usize,
}

/// Compile `issue_filters(request.GET, "GET", "issue__")` (`base.py:183`)
/// to SQL. Param parsing is the shared `pidash_db::issue_filters` kernel
/// (`issue_filters_get`); predicate compilation is transcribed from
/// `api/src/app_issues` `legacy_sql`/`legacy_text_sql` (private there)
/// against the real table names of the list query. Django `filter()`
/// joins are INNER; an `__isnull` lookup on a relation forces that
/// relation's join LEFT.
#[allow(clippy::result_large_err)]
fn compile_filter(query: &QueryMap, today: chrono::NaiveDate) -> Result<CompiledFilter, Response> {
    let mut params = std::collections::HashMap::new();
    for key in ISSUE_FILTER_KEYS {
        if let Some(last) = query_last(query, key) {
            params.insert((*key).to_owned(), last);
        }
    }
    let filter = match issue_filters_get(&params, "issue__", today) {
        Ok(filter) => filter,
        Err(IssueFilterError::DateOverflow) => return Err(Denial::ServerError.into_response()),
    };
    let predicates = filter.predicates();
    // Relation joins: LEFT when an __isnull lookup touches the relation,
    // else INNER (Django's filter()/exclude() join rule).
    let uses = |marker: &str| predicates.iter().any(|(name, _)| name.contains(marker));
    let isnull_uses = |marker: &str| {
        predicates
            .iter()
            .any(|(name, _)| name.contains(marker) && name.ends_with("__isnull"))
    };
    let mut joins = String::new();
    if uses("issue__state__group") {
        joins.push_str(" INNER JOIN \"states\" ON (\"states\".\"id\" = \"issues\".\"state_id\")");
    }
    if uses("assignees") || uses("issue_assignee") {
        let kind = if isnull_uses("assignees") || isnull_uses("issue_assignee") {
            "LEFT OUTER JOIN"
        } else {
            "INNER JOIN"
        };
        joins.push_str(&format!(
            " {kind} \"issue_assignees\" ON (\"issue_assignees\".\"issue_id\" = \"issues\".\"id\")"
        ));
    }
    if uses("issue_mention") {
        joins.push_str(
            " INNER JOIN \"issue_mentions\" ON (\"issue_mentions\".\"issue_id\" = \"issues\".\"id\")",
        );
    }
    if uses("issue_cycle") {
        let kind = if isnull_uses("issue_cycle") {
            "LEFT OUTER JOIN"
        } else {
            "INNER JOIN"
        };
        joins.push_str(&format!(
            " {kind} \"cycle_issues\" ON (\"cycle_issues\".\"issue_id\" = \"issues\".\"id\")"
        ));
    }
    if uses("issue_module") {
        let kind = if isnull_uses("issue_module") {
            "LEFT OUTER JOIN"
        } else {
            "INNER JOIN"
        };
        joins.push_str(&format!(
            " {kind} \"module_issues\" ON (\"module_issues\".\"issue_id\" = \"issues\".\"id\")"
        ));
    }
    if uses("issue_subscribers") {
        let kind = if isnull_uses("issue_subscribers") {
            "LEFT OUTER JOIN"
        } else {
            "INNER JOIN"
        };
        joins.push_str(&format!(
            " {kind} \"issue_subscribers\" ON (\"issue_subscribers\".\"issue_id\" = \"issues\".\"id\")"
        ));
    }
    // `label_issue`/`labels` ride the base query's LEFT joins.
    let mut binds = Vec::new();
    let mut next = 3usize;
    let mut parts = Vec::new();
    for (name, value) in predicates {
        match compile_predicate(name, value, &mut binds, &mut next)? {
            Some(sql) => parts.push(sql),
            None => return Err(Denial::ServerError.into_response()),
        }
    }
    let bind_count = binds.len();
    Ok(CompiledFilter {
        joins,
        fragment: if parts.is_empty() {
            None
        } else {
            Some(parts.join(" AND "))
        },
        binds,
        bind_count,
    })
}

/// Bind `$n` placeholders for one compiled predicate. `None` is the
/// `FieldError` arm (unknown column): Django raises and the 500 envelope
/// answers.
#[allow(clippy::result_large_err)]
fn compile_predicate(
    name: &str,
    value: &FilterValue,
    binds: &mut Vec<OwnedBind>,
    next: &mut usize,
) -> Result<Option<String>, Response> {
    // `__isnull` flags.
    if let Some(path) = name.strip_suffix("__isnull") {
        let column = match isnull_column(path) {
            Some(column) => column,
            None => return Ok(None),
        };
        let flag = match value {
            FilterValue::Flag(flag) => *flag,
            _ => return Err(Denial::ServerError.into_response()),
        };
        return Ok(Some(if flag {
            format!("{column} IS NULL")
        } else {
            format!("{column} IS NOT NULL")
        }));
    }
    match value {
        FilterValue::Uuids(ids) => {
            let column = match uuid_in_column(name) {
                Some(column) => column,
                None => return Ok(None),
            };
            if ids.is_empty() {
                return Ok(Some("FALSE".to_owned()));
            }
            let mut holders = Vec::with_capacity(ids.len());
            for id in ids {
                holders.push(placeholder(next));
                binds.push(OwnedBind::Uuid(*id));
            }
            Ok(Some(format!("{column} IN ({})", holders.join(","))))
        }
        FilterValue::Strings(items) => {
            let column = match strings_in_column(name) {
                Some(column) => column,
                None => return Ok(None),
            };
            if items.is_empty() {
                return Ok(Some("FALSE".to_owned()));
            }
            if name == "issue__issue_intake__status__in" {
                let mut numbers = Vec::with_capacity(items.len());
                for item in items {
                    match item.parse::<i32>() {
                        Ok(number) => numbers.push(number),
                        Err(_) => {
                            return Err(Denial::BadError("Please provide valid detail".to_owned())
                                .into_response());
                        }
                    }
                }
                let mut holders = Vec::with_capacity(numbers.len());
                for number in numbers {
                    holders.push(placeholder(next));
                    binds.push(OwnedBind::Int(number));
                }
                return Ok(Some(format!(
                    "EXISTS (SELECT 1 FROM \"intake_issues\" \"ii\" WHERE (\"ii\".\"issue_id\" = \"issues\".\"id\" AND \"ii\".\"status\" IN ({})))",
                    holders.join(",")
                )));
            }
            if name == "issue__estimate_point__in" {
                let mut holders = Vec::with_capacity(items.len());
                for item in items {
                    let id: Uuid = match item.parse() {
                        Ok(id) => id,
                        Err(_) => {
                            return Err(Denial::BadError("Please provide valid detail".to_owned())
                                .into_response());
                        }
                    };
                    holders.push(placeholder(next));
                    binds.push(OwnedBind::Uuid(id));
                }
                return Ok(Some(format!("{column} IN ({})", holders.join(","))));
            }
            let mut holders = Vec::with_capacity(items.len());
            for item in items {
                holders.push(placeholder(next));
                binds.push(OwnedBind::Text(item.clone()));
            }
            Ok(Some(format!("{column} IN ({})", holders.join(","))))
        }
        FilterValue::Text(text) => compile_text_predicate(name, text, binds, next),
        FilterValue::Flag(_) => Err(Denial::ServerError.into_response()),
        FilterValue::Day(day) => {
            let (column, operator) = match day_comparison(name) {
                Some(pair) => pair,
                None => return Ok(None),
            };
            let holder = placeholder(next);
            binds.push(OwnedBind::Date(*day));
            Ok(Some(format!("{column} {operator} {holder}::date")))
        }
        FilterValue::Null => {
            let column = match isnull_column(name) {
                Some(column) => column,
                None => return Ok(None),
            };
            Ok(Some(format!("{column} IS NULL")))
        }
    }
}

fn placeholder(next: &mut usize) -> String {
    let holder = format!("${next}");
    *next += 1;
    holder
}

/// Columns for `__isnull` predicates (and bare `Null` values), real table
/// names for the list query.
fn isnull_column(path: &str) -> Option<&'static str> {
    Some(match path {
        "issue__parent" => "\"issues\".\"parent_id\"",
        "issue__labels" => "\"issue_labels\".\"label_id\"",
        "issue__assignees" => "\"issue_assignees\".\"assignee_id\"",
        "issue__created_by" => "\"issues\".\"created_by_id\"",
        "issue__issue_cycle__cycle_id" => "\"cycle_issues\".\"cycle_id\"",
        "issue__issue_module__module_id" => "\"module_issues\".\"module_id\"",
        "issue__label_issue__deleted_at" => "\"issue_labels\".\"deleted_at\"",
        "issue__issue_assignee__deleted_at" => "\"issue_assignees\".\"deleted_at\"",
        "issue__issue_cycle__deleted_at" => "\"cycle_issues\".\"deleted_at\"",
        "issue__issue_module__deleted_at" => "\"module_issues\".\"deleted_at\"",
        "issue__issue_subscribers__deleted_at" => "\"issue_subscribers\".\"deleted_at\"",
        "issue__target_date" => "\"issues\".\"target_date\"",
        "issue__start_date" => "\"issues\".\"start_date\"",
        _ => return None,
    })
}

/// Columns for UUID `__in` predicates. `logged_by` has no model field:
/// Django raises `FieldError` (generic 500).
fn uuid_in_column(name: &str) -> Option<&'static str> {
    Some(match name {
        "issue__state__in" => "\"issues\".\"state_id\"",
        "issue__parent__in" => "\"issues\".\"parent_id\"",
        "issue__labels__in" => "\"issue_labels\".\"label_id\"",
        "issue__assignees__in" => "\"issue_assignees\".\"assignee_id\"",
        "issue__issue_mention__mention__id__in" => "\"issue_mentions\".\"mention_id\"",
        "issue__created_by__in" => "\"issues\".\"created_by_id\"",
        "issue__project__in" => "\"issues\".\"project_id\"",
        "issue__issue_cycle__cycle_id__in" => "\"cycle_issues\".\"cycle_id\"",
        "issue__issue_module__module_id__in" => "\"module_issues\".\"module_id\"",
        "issue__issue_subscribers__subscriber_id__in" => "\"issue_subscribers\".\"subscriber_id\"",
        _ => return None,
    })
}

/// Columns for string `__in` predicates.
fn strings_in_column(name: &str) -> Option<&'static str> {
    Some(match name {
        "issue__state__group__in" => "\"states\".\"group\"",
        "issue__estimate_point__in" => "\"issues\".\"estimate_point_id\"",
        "issue__priority__in" => "\"issues\".\"priority\"",
        "issue__issue_intake__status__in" => "\"intake_issues\".\"status\"",
        _ => return None,
    })
}

/// `(column, operator)` for `Day` predicates (`__gte` / `__lte`).
fn day_comparison(name: &str) -> Option<(&'static str, &'static str)> {
    let (term, operator) = name.rsplit_once("__")?;
    let column = match term {
        "issue__created_at__date" => "\"issues\".\"created_at\"::date",
        "issue__completed_at__date" => "\"issues\".\"completed_at\"::date",
        "issue__start_date" => "\"issues\".\"start_date\"",
        "issue__target_date" => "\"issues\".\"target_date\"",
        _ => return None,
    };
    let operator = match operator {
        "gte" => ">=",
        "lte" => "<=",
        _ => return None,
    };
    Some((column, operator))
}

/// Text predicates: `name__icontains`, explicit date bounds
/// (`__gte`/`__lte` on a `__date` term or plain date term), and the
/// single-value `__contains` form.
#[allow(clippy::result_large_err)]
fn compile_text_predicate(
    name: &str,
    text: &str,
    binds: &mut Vec<OwnedBind>,
    next: &mut usize,
) -> Result<Option<String>, Response> {
    if name == "issue__name__icontains" {
        // Django `icontains`: LIKE with `\`, `%`, `_` escaped.
        let escaped = text
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let holder = placeholder(next);
        binds.push(OwnedBind::Text(format!("%{escaped}%")));
        return Ok(Some(format!("\"issues\".\"name\" ILIKE {holder}")));
    }
    let Some((term, operator)) = name.rsplit_once("__") else {
        return Ok(None);
    };
    let operator = match operator {
        "gte" => ">=",
        "lte" => "<=",
        "contains" => "=",
        _ => return Ok(None),
    };
    let column = match term {
        "issue__created_at__date" => "\"issues\".\"created_at\"::date",
        "issue__completed_at__date" => "\"issues\".\"completed_at\"::date",
        "issue__start_date" => "\"issues\".\"start_date\"",
        "issue__target_date" => "\"issues\".\"target_date\"",
        _ => return Ok(None),
    };
    // Explicit bounds must parse as dates: Django's `get_prep_value`
    // raises `ValidationError` (400 invalid detail) on garbage.
    if text.parse::<chrono::NaiveDate>().is_err() && parse_datetime_param(text).is_none() {
        return Err(Denial::BadError("Please provide valid detail".to_owned()).into_response());
    }
    if operator == "=" {
        // Single-value form on a date term: Django's `contains`
        // lookup, i.e. LIKE with metacharacters escaped.
        let escaped = text
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let holder = placeholder(next);
        binds.push(OwnedBind::Text(format!("%{escaped}%")));
        return Ok(Some(format!("{column}::text LIKE {holder}")));
    }
    let holder = placeholder(next);
    binds.push(OwnedBind::Text(text.to_owned()));
    Ok(Some(format!("{column} {operator} {holder}::date")))
}

/// Parse an `updated_at__gt`-style datetime param the way Django's
/// `DateTimeField.get_prep_value` does (naive values attach UTC).
/// Garbage is a `ValidationError`, not SQL text.
fn parse_datetime_param(text: &str) -> Option<DateTime<Utc>> {
    if let Ok(aware) = DateTime::parse_from_rfc3339(text) {
        return Some(aware.with_timezone(&Utc));
    }
    for format in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%m/%d/%Y %H:%M:%S",
        "%m/%d/%Y %H:%M",
        "%m/%d/%y %H:%M:%S",
        "%m/%d/%y %H:%M",
        "%Y-%m-%d",
        "%m/%d/%Y",
        "%m/%d/%y",
    ] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(text, format) {
            return Some(naive.and_utc());
        }
        if let Ok(date) = chrono::NaiveDate::parse_from_str(text, format) {
            return Some(date.and_hms_opt(0, 0, 0)?.and_utc());
        }
    }
    None
}

/// Chain owned binds into a row-fetch statement.
fn bind_owned<'q>(
    mut statement: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    binds: &'q [OwnedBind],
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    for bind in binds {
        statement = match bind {
            OwnedBind::Uuid(id) => statement.bind(*id),
            OwnedBind::Text(text) => statement.bind(text.clone()),
            OwnedBind::Int(number) => statement.bind(*number),
            OwnedBind::Date(day) => statement.bind(*day),
        };
    }
    statement
}

/// Chain owned binds into a scalar statement.
fn bind_owned_scalar<'q>(
    mut statement: sqlx::query::QueryScalar<'q, sqlx::Postgres, i64, sqlx::postgres::PgArguments>,
    binds: &'q [OwnedBind],
) -> sqlx::query::QueryScalar<'q, sqlx::Postgres, i64, sqlx::postgres::PgArguments> {
    for bind in binds {
        statement = match bind {
            OwnedBind::Uuid(id) => statement.bind(*id),
            OwnedBind::Text(text) => statement.bind(text.clone()),
            OwnedBind::Int(number) => statement.bind(*number),
            OwnedBind::Date(day) => statement.bind(*day),
        };
    }
    statement
}

// ---------------------------------------------------------------------------
// list rendering: IntakeIssueSerializer rows
// ---------------------------------------------------------------------------

/// The issue columns the list's nested `IssueIntakeSerializer` renders
/// (`issue.py:1026-1035`).
struct ListIssue {
    id: Uuid,
    name: String,
    priority: String,
    sequence_id: i32,
    project_id: Uuid,
    created_at: DateTime<Utc>,
    created_by_id: Option<Uuid>,
}

/// One `IN` fetch for the page's issues (what `select_related("issue")`
/// folds into the Django query).
async fn fetch_list_issues(
    pool: &PgPool,
    ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, ListIssue>, sqlx::Error> {
    let mut out = std::collections::HashMap::with_capacity(ids.len());
    if ids.is_empty() {
        return Ok(out);
    }
    // Dynamically-sized `IN` lists cannot bind as one placeholder with
    // sqlx; the ids are server-generated UUIDs already read back from
    // the page query, so inline them as literals.
    let list = ids
        .iter()
        .map(|id| format!("'{id}'"))
        .collect::<Vec<_>>()
        .join(",");
    let rows = sqlx::query(&format!(
        "SELECT \"id\", \"name\", \"priority\", \"sequence_id\", \"project_id\", \"created_at\", \"created_by_id\" FROM \"issues\" WHERE (\"id\" IN ({list}))"
    ))
    .fetch_all(pool)
    .await?;
    for row in rows {
        let issue = ListIssue {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            priority: row.try_get("priority")?,
            sequence_id: row.try_get("sequence_id")?,
            project_id: row.try_get("project_id")?,
            created_at: row.try_get("created_at")?,
            created_by_id: row.try_get("created_by_id")?,
        };
        out.insert(issue.id, issue);
    }
    Ok(out)
}

/// `IntakeIssueSerializer` row (`intake.py:27-41` + `to_representation`
/// `:86-90`): explicit `Meta.fields` order; the annotated `label_ids`
/// ride the nested issue object. Decode errors propagate: every column
/// is projected by the list query, so a failure is a programmer error
/// (a wrong column name rendered `duplicate_to` as null until
/// PIDASHCONV-1434) and must 500, never render a default.
fn render_list_row(
    row: &sqlx::postgres::PgRow,
    issue: &ListIssue,
    timezone: chrono_tz::Tz,
) -> Result<serde_json::Value, sqlx::Error> {
    let label_ids: Vec<Uuid> = row.try_get("label_ids")?;
    let mut nested = serde_json::Map::with_capacity(8);
    nested.insert("id".to_owned(), uuid_string(&issue.id));
    nested.insert(
        "name".to_owned(),
        serde_json::Value::String(issue.name.clone()),
    );
    nested.insert(
        "priority".to_owned(),
        serde_json::Value::String(issue.priority.clone()),
    );
    nested.insert(
        "sequence_id".to_owned(),
        serde_json::Value::from(issue.sequence_id),
    );
    nested.insert("project_id".to_owned(), uuid_string(&issue.project_id));
    nested.insert(
        "created_at".to_owned(),
        serde_json::Value::String(crate::serializer::render_datetime_in(
            &issue.created_at,
            &timezone,
        )),
    );
    nested.insert(
        "label_ids".to_owned(),
        serde_json::Value::Array(label_ids.into_iter().map(|id| uuid_string(&id)).collect()),
    );
    nested.insert("created_by".to_owned(), opt_uuid(&issue.created_by_id));
    let mut body = serde_json::Map::with_capacity(7);
    let id: Uuid = row.try_get("id")?;
    body.insert("id".to_owned(), serde_json::Value::String(id.to_string()));
    let status: i32 = row.try_get("status")?;
    body.insert("status".to_owned(), serde_json::Value::from(status));
    let duplicate_to: Option<Uuid> = row.try_get("duplicate_to_id")?;
    body.insert("duplicate_to".to_owned(), opt_uuid(&duplicate_to));
    let snoozed_till: Option<DateTime<Utc>> = row.try_get("snoozed_till")?;
    body.insert("snoozed_till".to_owned(), opt_dt(&snoozed_till, timezone));
    let source: Option<String> = row.try_get("source")?;
    body.insert("source".to_owned(), opt_string(&source));
    body.insert("issue".to_owned(), serde_json::Value::Object(nested));
    let created_by: Option<Uuid> = row.try_get("created_by_id")?;
    body.insert("created_by".to_owned(), opt_uuid(&created_by));
    Ok(serde_json::Value::Object(body))
}

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

/// `create` (`base.py:222-326`).
pub async fn collection_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::body::Bytes,
) -> Response {
    let gate = match resolve_gate(&state, &slug, &project_raw, extension).await {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let mut data = match parse_body(&body) {
        Ok(data) => data,
        Err(response) => return response,
    };
    // `request.data.get("issue", {})`: missing → `{}`; a non-dict raises
    // before the name gate (500) — and so does an explicit `null`.
    let issue_value = data
        .get("issue")
        .cloned()
        .unwrap_or(serde_json::Value::Object(Default::default()));
    let serde_json::Value::Object(issue_data) = issue_value else {
        return Denial::ServerError.into_response();
    };
    // `:223-224`: falsy name (missing, `""`, null, false, 0) → 400.
    let name_value = issue_data
        .get("name")
        .cloned()
        .unwrap_or(serde_json::Value::Bool(false));
    if !json_truthy(&name_value) {
        return Denial::BadError("Name is required".to_owned()).into_response();
    }
    // `:227-234`: priority defaults to `"none"`; a present non-string
    // (including null) denies, as does a string outside the allowlist.
    // The value itself flows through the serializer below.
    // (A bare match guard would fall through to the catch-all on
    // valid strings; the if/else pins the pass case.)
    if let Some(serde_json::Value::String(priority)) = issue_data.get("priority") {
        if !["low", "medium", "high", "urgent", "none"].contains(&priority.as_str()) {
            return Denial::BadError("Invalid priority".to_owned()).into_response();
        }
    } else if issue_data.get("priority").is_some() {
        return Denial::BadError("Invalid priority".to_owned()).into_response();
    };
    // Project context for the serializer (`:236,255-261`): workspace id
    // (backfilled from the project) and the default assignee.
    let project_row: Option<(Option<Uuid>,)> = match sqlx::query_as(
        "SELECT \"projects\".\"default_assignee_id\" FROM \"projects\" WHERE (\"projects\".\"id\" = $1)",
    )
    .bind(gate.project_id)
    .fetch_optional(pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some((default_assignee_id,)) = project_row else {
        return Denial::ServerError.into_response();
    };
    // Triage get-or-create (`:239-249`): the triage-manager lookup scopes
    // project + workspace slug; `State.save` resequences to
    // `max(non-triage project states) + 15000`, keeping the passed
    // `65000` only when no non-triage state exists.
    let triage_id = match ensure_triage_state(pool, &gate).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    // `request.data["issue"]["state_id"] = triage_state.id` (`:250`):
    // the mutation lands in the serializer input AND in both Celery
    // dumps below (UUIDs render as strings, like `DjangoJSONEncoder`).
    // Mutate `data` itself so the dumps see it; re-clone the input.
    if let Some(issue_obj) = data.get_mut("issue").and_then(|v| v.as_object_mut()) {
        issue_obj.insert(
            "state_id".to_owned(),
            serde_json::Value::String(triage_id.to_string()),
        );
    }
    let serde_json::Value::Object(issue_data) = data.get("issue").cloned().unwrap_or_default()
    else {
        return Denial::ServerError.into_response();
    };
    // `IssueCreateSerializer(...).is_valid()` (`:253-263`): the reachable
    // field validations; serializer errors answer 400 (`:325-326`).
    let validated = match validate_issue_create(
        pool,
        &gate,
        state.settings(),
        &issue_data,
        default_assignee_id,
    )
    .await
    {
        Ok(validated) => validated,
        Err(response) => return response,
    };
    let now = Utc::now();
    // `assigned_pod`: explicit input wins, else the project default pod
    // (`Issue.save`, `:277-286`) — resolved before the atomic block, like
    // Django. It must not run on the pool after `begin`: the transaction
    // holds one pool connection while the lock serializes creators, so a
    // nested pool checkout would exhaust the pool under concurrent load.
    let assigned_pod_id = match validated.assigned_pod_id {
        Some(pod) => Some(pod),
        None => match default_project_pod(pool, gate.project_id).await {
            Ok(pod) => pod,
            Err(response) => return response,
        },
    };
    // `Issue.save` (`db/models/issue.py:313-343`): the transaction-scoped
    // advisory lock is taken first, then MAX(sequence) and MAX(sort_order)
    // are read and the issue plus its sequence row are inserted — all on
    // one transaction, so concurrent creates serialize per project. The
    // lock must live on the transaction: taken on the pool it would drop
    // as soon as its own autocommit statement ends (PIDASHCONV-1430).
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Denial::ServerError.into_response(),
    };
    if sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(advisory_key(&gate.project_id))
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return Denial::ServerError.into_response();
    }
    let sequence_id: i32 = match sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(\"sequence\") FROM \"issue_sequences\" WHERE (\"issue_sequences\".\"project_id\" = $1)",
    )
    .bind(gate.project_id)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(max) => max.map(|max| max + 1).unwrap_or(1) as i32,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let sort_order: f64 = match sqlx::query_scalar::<_, Option<f64>>(
        "SELECT MAX(\"sort_order\") FROM \"issues\" WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"project_id\" = $1 AND \"issues\".\"state_id\" = $2)",
    )
    .bind(gate.project_id)
    .bind(triage_id)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(max) => max.map(|max| max + 10000.0).unwrap_or(65535.0),
        Err(_) => return Denial::ServerError.into_response(),
    };
    // `save` keeps an input sort order only when no max exists; the
    // EXISTS check runs on the transaction like Django's MAX read.
    let sort_order = match validated.sort_order {
        Some(input) => {
            let has_max: bool = match sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM \"issues\" WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"project_id\" = $1 AND \"issues\".\"state_id\" = $2))",
            )
            .bind(gate.project_id)
            .bind(triage_id)
            .fetch_one(&mut *tx)
            .await
            {
                Ok(has_max) => has_max,
                Err(_) => return Denial::ServerError.into_response(),
            };
            if has_max {
                sort_order
            } else {
                input
            }
        }
        None => sort_order,
    };
    let issue_id = Uuid::new_v4();
    // `IssueCreateSerializer.create` (`serializers/issue.py:290-371`) +
    // `Issue.save` + `BaseModel.save` (crum `created_by`): the full row.
    if let Err(response) = insert_issue(
        &mut tx,
        &gate,
        &validated,
        issue_id,
        triage_id,
        sequence_id,
        sort_order,
        now,
        assigned_pod_id,
    )
    .await
    {
        return response;
    }
    if let Err(error) = sqlx::query(
        "INSERT INTO \"issue_sequences\" (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"project_id\", \"workspace_id\", \"issue_id\", \"sequence\", \"deleted\") VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, FALSE)",
    )
    .bind(Uuid::new_v4())
    .bind(now)
    .bind(now)
    .bind(gate.user_id)
    .bind(gate.project_id)
    .bind(gate.workspace_id)
    .bind(issue_id)
    .bind(i64::from(sequence_id))
    .execute(&mut *tx)
    .await
    {
        return db_error(error);
    }
    // The M2M writes, the bridge insert and the enqueues below run after
    // the commit: Django's view is not atomic beyond `Issue.save`, so they
    // stay outside this transaction (`serializers/issue.py:311-369`,
    // `views/intake/base.py:265-292`).
    if tx.commit().await.is_err() {
        return Denial::ServerError.into_response();
    }
    if let Err(response) =
        insert_issue_m2m(pool, &gate, &validated, issue_id, now, default_assignee_id).await
    {
        return response;
    }
    // `IntakeIssue.objects.create(...)` (`:265-272`): the intake id is
    // the FIRST intake row for the project (name ordering); the
    // `.first()` dereference is unguarded, so a missing row is a 500
    // (QUIRK-create-no-intake).
    let intake_row: Option<Uuid> = match sqlx::query_scalar(
        "SELECT \"intakes\".\"id\" FROM \"intakes\" WHERE (\"intakes\".\"workspace_id\" = $1 AND \"intakes\".\"project_id\" = $2 AND \"intakes\".\"deleted_at\" IS NULL) ORDER BY \"intakes\".\"name\" ASC LIMIT 1",
    )
    .bind(gate.workspace_id)
    .bind(gate.project_id)
    .fetch_optional(pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some(first_intake_id) = intake_row else {
        return Denial::ServerError.into_response();
    };
    let bridge_id = Uuid::new_v4();
    if let Err(error) = sqlx::query(
        "INSERT INTO \"intake_issues\" (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"project_id\", \"workspace_id\", \"intake_id\", \"issue_id\", \"status\", \"snoozed_till\", \"duplicate_to_id\", \"source\", \"source_email\", \"external_source\", \"external_id\", \"extra\") VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}')",
    )
    .bind(bridge_id)
    .bind(now)
    .bind(now)
    .bind(gate.user_id)
    .bind(gate.project_id)
    .bind(gate.workspace_id)
    .bind(first_intake_id)
    .bind(issue_id)
    .execute(pool)
    .await
    {
        return db_error(error);
    }
    // Emits (`:274-292`): `requested_data`/`updated_issue` dump the full
    // (mutated) request JSON; `current_instance` is `None`; the origin is
    // `base_host(is_app=True)`.
    let epoch = Utc::now().timestamp();
    let dump = super::python_dumps(&data);
    let origin = match app_origin(&state) {
        Some(origin) => origin,
        None => return Denial::ServerError.into_response(),
    };
    let activity = intake_tasks::intake_create_activity(
        dump.clone(),
        gate.user_id.to_string(),
        issue_id.to_string(),
        gate.project_id.to_string(),
        epoch,
        origin,
        bridge_id.to_string(),
    );
    enqueue_message(
        pool,
        pidash_jobs::TaskMessage::new(activity.task_name(), vec![], activity.kwargs()),
    )
    .await;
    // v1 description version (`:287-292`): enqueue only, like Django.
    // No synchronous row insert (PIDASHCONV-983; the PIDASHCONV-500 sync
    // quirk is gone — the row appears when the worker runs, and the
    // versions tests poll for it).
    let version = intake_tasks::intake_create_description_version(
        dump,
        issue_id.to_string(),
        gate.user_id.to_string(),
    );
    enqueue_message(
        pool,
        pidash_jobs::TaskMessage::new(version.task_name(), vec![], version.kwargs()),
    )
    .await;
    // Re-fetch with the label/assignee annotates (`:293-322`) and render
    // the detail (`:323-324`).
    match render_created_detail(pool, &gate, first_intake_id, issue_id, bridge_id).await {
        Ok(body) => raw_json_response(body),
        Err(response) => response,
    }
}

// ---------------------------------------------------------------------------
// create: triage state
// ---------------------------------------------------------------------------

/// Triage get-or-create (`base.py:239-249`).
///
/// `State.triage_objects.filter(project_id, workspace__slug).first()`:
/// the triage manager keeps `group = 'triage'` rows; `Meta.ordering` is
/// `sequence`. On a miss the row is created with `State.save` semantics:
/// `slug` from the name and `sequence = max(non-triage project states) +
/// 15000`, keeping the passed `65000` when no non-triage state exists
/// (the max query runs over `State.objects`, which excludes triage).
#[allow(clippy::result_large_err)]
async fn ensure_triage_state(
    pool: &PgPool,
    gate: &crate::app_issues::Gate,
) -> Result<Uuid, Response> {
    let existing: Option<Uuid> = match sqlx::query_scalar(
        "SELECT \"states\".\"id\" FROM \"states\" INNER JOIN \"workspaces\" ON (\"states\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"states\".\"project_id\" = $1 AND \"workspaces\".\"slug\" = $2 AND \"states\".\"group\" = 'triage' AND \"states\".\"deleted_at\" IS NULL) ORDER BY \"states\".\"sequence\" ASC LIMIT 1",
    )
    .bind(gate.project_id)
    .bind(
        sqlx::query_scalar::<_, String>("SELECT \"slug\" FROM \"workspaces\" WHERE (\"id\" = $1)")
            .bind(gate.workspace_id)
            .fetch_one(pool)
            .await
            .map_err(|_| Denial::ServerError.into_response())?,
    )
    .fetch_optional(pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return Err(Denial::ServerError.into_response()),
    };
    if let Some(id) = existing {
        return Ok(id);
    }
    let max_sequence: Option<f64> = match sqlx::query_scalar(
        "SELECT MAX(\"sequence\") FROM \"states\" WHERE (\"states\".\"project_id\" = $1 AND \"states\".\"group\" != 'triage' AND \"states\".\"deleted_at\" IS NULL)",
    )
    .bind(gate.project_id)
    .fetch_one(pool)
    .await
    {
        Ok(max) => max,
        Err(_) => return Err(Denial::ServerError.into_response()),
    };
    let sequence = max_sequence.map(|max| max + 15000.0).unwrap_or(65000.0);
    let new_id = Uuid::new_v4();
    let now = Utc::now();
    // Full `State.objects.create` row: explicit kwargs plus the
    // `save()`-filled columns (`description=""`, `slug="triage"`,
    // `is_triage=False`) and the crum `created_by`.
    if let Err(error) = sqlx::query(
        "INSERT INTO \"states\" (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"name\", \"description\", \"slug\", \"color\", \"project_id\", \"workspace_id\", \"sequence\", \"group\", \"default\", \"is_triage\") VALUES ($1, $2, $3, $4, 'Triage', '', 'triage', '#4E5355', $5, $6, $7, 'triage', false, false)",
    )
    .bind(new_id)
    .bind(now)
    .bind(now)
    .bind(gate.user_id)
    .bind(gate.project_id)
    .bind(gate.workspace_id)
    .bind(sequence)
    .execute(pool)
    .await
    {
        return Err(db_error(error));
    }
    Ok(new_id)
}

/// `convert_uuid_to_integer(project.id)` (`utils/uuid.py:19-26`): sha256
/// of the string form, first 8 bytes big-endian signed. The lock
/// serializes sequence assignment (`db/models/issue.py:318`).
fn advisory_key(project_id: &Uuid) -> i64 {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(project_id.to_string().as_bytes());
    i64::from_be_bytes(digest[..8].try_into().expect("eight bytes"))
}

/// `base_host(request, is_app=True)` (`utils/host.py:61-65`):
/// `APP_BASE_URL` when set, else the `WEB_URL or APP_BASE_URL` origin.
/// Both unset is `ImproperlyConfigured` (500).
fn app_origin(state: &AppState) -> Option<String> {
    let settings = state.settings();
    if let Some(base) = &settings.urls.app_base_url {
        return Some(base.clone());
    }
    if let Some(web) = &settings.urls.web_url {
        return Some(web.clone());
    }
    settings.urls.app_base_url.clone()
}

/// Map a database failure onto the `handle_exception` matrix: integrity
/// violations (SQLSTATE 23xxx) → 400; anything else → the 500 envelope.
fn db_error(error: sqlx::Error) -> Response {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.code().as_deref().unwrap_or("").starts_with("23") {
            return Denial::BadError("The payload is not valid".to_owned()).into_response();
        }
    }
    Denial::ServerError.into_response()
}

// ---------------------------------------------------------------------------
// bodies and scalars
// ---------------------------------------------------------------------------

fn uuid_string(id: &Uuid) -> serde_json::Value {
    serde_json::Value::String(id.to_string())
}

// ---------------------------------------------------------------------------
// create: IssueCreateSerializer validation
// ---------------------------------------------------------------------------

/// The validated issue attributes (`IssueCreateSerializer.is_valid`,
/// `serializers/issue.py:136-385`): DRF field order for error bodies,
/// then the `validate()` checks in code order. Only reachable validations
/// are implemented; every arm cites its source.
#[derive(Debug, Clone)]
struct ValidatedCreate {
    parent_id: Option<Uuid>,
    assigned_pod_id: Option<Uuid>,
    label_ids: Option<Vec<Uuid>>,
    assignee_ids: Option<Vec<Uuid>>,
    point: Option<i32>,
    name: String,
    description_json: serde_json::Value,
    description_html: Option<String>,
    priority: String,
    complexity_score: i32,
    start_date: Option<chrono::NaiveDate>,
    target_date: Option<chrono::NaiveDate>,
    // Validated then discarded: `Issue.save` always overrides
    // `sequence_id` with `MAX+1`.
    #[allow(dead_code)]
    sequence_id: Option<i32>,
    sort_order: Option<f64>,
    // Validated then discarded: `Issue.save` clears `completed_at`
    // whenever the state group is not completed, and this path always
    // assigns the triage state.
    #[allow(dead_code)]
    completed_at: Option<DateTime<Utc>>,
    archived_at: Option<chrono::NaiveDate>,
    is_draft: bool,
    external_source: Option<String>,
    external_id: Option<String>,
    git_work_branch: String,
    created_via: Option<String>,
    agent_executor: Option<String>,
    estimate_point_id: Option<Uuid>,
    type_id: Option<Uuid>,
    deleted_at: Option<DateTime<Utc>>,
}

/// Field-level validation (`to_internal_value` for each writable field,
/// `serializers/issue.py:136-204`): collects every field error before
/// `validate()` runs. `issue_data` already carries the server-set
/// `state_id`.
#[allow(clippy::result_large_err)]
async fn validate_issue_create(
    pool: &PgPool,
    gate: &crate::app_issues::Gate,
    settings: &pidash_db::config::Settings,
    issue_data: &serde_json::Map<String, serde_json::Value>,
    _default_assignee_id: Option<Uuid>,
) -> Result<ValidatedCreate, Response> {
    let mut field_errors = serde_json::Map::new();
    // `state_id` is server-set to the triage row (`base.py:250`), which
    // the triage lookup just ensured — the PK + project checks below
    // always pass on this path.
    let triage_id = match issue_data.get("state_id") {
        Some(serde_json::Value::String(raw)) => raw.parse::<Uuid>().ok(),
        _ => None,
    };
    // Declared-field order first: state_id, parent_id, assigned_pod_id,
    // label_ids, assignee_ids.
    let parent_id = match opt_pk_field(issue_data, "parent_id", pool, "issues", true).await {
        Ok(id) => id,
        Err(message) => {
            field_errors.insert("parent_id".to_owned(), serde_json::json!([message]));
            None
        }
    };
    let assigned_pod_id =
        match opt_pk_field(issue_data, "assigned_pod_id", pool, "pod", false).await {
            Ok(id) => id,
            Err(message) => {
                field_errors.insert("assigned_pod_id".to_owned(), serde_json::json!([message]));
                None
            }
        };
    let label_ids = match opt_uuid_list_field(issue_data, "label_ids", pool, "labels", true).await {
        Ok(ids) => ids,
        Err(body) => {
            field_errors.insert("label_ids".to_owned(), body);
            None
        }
    };
    let assignee_ids =
        match opt_uuid_list_field(issue_data, "assignee_ids", pool, "users", false).await {
            Ok(ids) => ids,
            Err(body) => {
                field_errors.insert("assignee_ids".to_owned(), body);
                None
            }
        };
    // Auto fields in `Meta` order: point, name, description_json,
    // description_html, priority (already gated), complexity_score,
    // dates, sequence_id, sort_order, completed_at, archived_at,
    // is_draft, external_*, git_work_branch, created_via,
    // agent_executor, estimate_point, type, deleted_at.
    let point = match opt_int_field(issue_data, "point") {
        Ok(value) => {
            if let Some(point) = value {
                if !(0..=12).contains(&point) {
                    field_errors.insert(
                        "point".to_owned(),
                        serde_json::json!([if point < 0 {
                            "Ensure this value is greater than or equal to 0."
                        } else {
                            "Ensure this value is less than or equal to 12."
                        }]),
                    );
                }
            }
            value.filter(|point| (0..=12).contains(point))
        }
        Err(message) => {
            field_errors.insert("point".to_owned(), serde_json::json!([message]));
            None
        }
    };
    // `name` passed the view's truthiness gate; the serializer trims and
    // applies blank/max_length (`CharField`, `max_length=255`).
    let name = match issue_data.get("name") {
        Some(value) => match char_internal(value) {
            Ok(name) if name.is_empty() => {
                field_errors.insert(
                    "name".to_owned(),
                    serde_json::json!(["This field may not be blank."]),
                );
                String::new()
            }
            Ok(name) if name.chars().count() > 255 => {
                field_errors.insert(
                    "name".to_owned(),
                    serde_json::json!(["Ensure this field has no more than 255 characters."]),
                );
                String::new()
            }
            Ok(name) => name,
            Err(()) => {
                let message = if value.is_null() {
                    "This field may not be null."
                } else {
                    "Not a valid string."
                };
                field_errors.insert("name".to_owned(), serde_json::json!([message]));
                String::new()
            }
        },
        None => {
            field_errors.insert(
                "name".to_owned(),
                serde_json::json!(["This field is required."]),
            );
            String::new()
        }
    };
    let description_json = match issue_data.get("description_json") {
        None => serde_json::json!({}),
        Some(serde_json::Value::Null) => {
            field_errors.insert(
                "description_json".to_owned(),
                serde_json::json!(["This field may not be null."]),
            );
            serde_json::json!({})
        }
        Some(value) => value.clone(),
    };
    // `TextField(blank=True, default="<p></p>")`: missing → default;
    // null → 400; anything else trims like `CharField`.
    let description_html = match issue_data.get("description_html") {
        None => Some(crate::space::sanitize::DEFAULT_DESCRIPTION_HTML.to_owned()),
        Some(serde_json::Value::Null) => {
            field_errors.insert(
                "description_html".to_owned(),
                serde_json::json!(["This field may not be null."]),
            );
            None
        }
        Some(value) => match char_internal(value) {
            Ok(html) => Some(html),
            Err(()) => {
                let message = if value.is_null() {
                    "This field may not be null."
                } else {
                    "Not a valid string."
                };
                field_errors.insert("description_html".to_owned(), serde_json::json!([message]));
                None
            }
        },
    };
    let priority = match issue_data.get("priority") {
        Some(serde_json::Value::String(priority)) => priority.clone(),
        _ => "none".to_owned(),
    };
    // No `null=True` on these integers: explicit null fails.
    for key in ["complexity_score", "sequence_id"] {
        if issue_data.get(key) == Some(&serde_json::Value::Null) {
            field_errors.insert(
                key.to_owned(),
                serde_json::json!(["This field may not be null."]),
            );
        }
    }
    if issue_data.get("sort_order") == Some(&serde_json::Value::Null) {
        field_errors.insert(
            "sort_order".to_owned(),
            serde_json::json!(["This field may not be null."]),
        );
    }
    // `complexity_score` carries model `MinValueValidator(0)` /
    // `MaxValueValidator(10)`: DRF runs field validators during
    // field-level validation, so their messages win and
    // `validate_complexity_score` (same bounds, custom message) is
    // unreachable dead code on this path.
    let complexity_score = match opt_int_field(issue_data, "complexity_score") {
        Ok(value) => {
            if let Some(score) = value {
                if score < 0 {
                    field_errors.insert(
                        "complexity_score".to_owned(),
                        serde_json::json!(["Ensure this value is greater than or equal to 0."]),
                    );
                } else if score > 10 {
                    field_errors.insert(
                        "complexity_score".to_owned(),
                        serde_json::json!(["Ensure this value is less than or equal to 10."]),
                    );
                }
            }
            value.filter(|score| (0..=10).contains(score)).unwrap_or(0)
        }
        Err(message) => {
            field_errors.insert("complexity_score".to_owned(), serde_json::json!([message]));
            0
        }
    };
    let start_date = match opt_date_field(issue_data, "start_date") {
        Ok(value) => value,
        Err(message) => {
            field_errors.insert("start_date".to_owned(), serde_json::json!([message]));
            None
        }
    };
    let target_date = match opt_date_field(issue_data, "target_date") {
        Ok(value) => value,
        Err(message) => {
            field_errors.insert("target_date".to_owned(), serde_json::json!([message]));
            None
        }
    };
    let sequence_id = match opt_int_field(issue_data, "sequence_id") {
        Ok(value) => value,
        Err(message) => {
            field_errors.insert("sequence_id".to_owned(), serde_json::json!([message]));
            None
        }
    };
    let sort_order = match opt_float_field(issue_data, "sort_order") {
        Ok(value) => value,
        Err(message) => {
            field_errors.insert("sort_order".to_owned(), serde_json::json!([message]));
            None
        }
    };
    let completed_at = match opt_datetime_field(issue_data, "completed_at") {
        Ok(value) => value,
        Err(message) => {
            field_errors.insert("completed_at".to_owned(), serde_json::json!([message]));
            None
        }
    };
    let archived_at = match opt_date_field(issue_data, "archived_at") {
        Ok(value) => value,
        Err(message) => {
            field_errors.insert("archived_at".to_owned(), serde_json::json!([message]));
            None
        }
    };
    // `is_draft` has no `null=True`: explicit null fails (missing → False).
    let is_draft = match issue_data.get("is_draft") {
        None => false,
        Some(serde_json::Value::Null) => {
            field_errors.insert(
                "is_draft".to_owned(),
                serde_json::json!(["This field may not be null."]),
            );
            false
        }
        Some(_) => match opt_bool_field(issue_data, "is_draft") {
            Ok(value) => value.unwrap_or(false),
            Err(message) => {
                field_errors.insert("is_draft".to_owned(), serde_json::json!([message]));
                false
            }
        },
    };
    let external_source = match opt_limited_string(issue_data, "external_source", 255) {
        Ok(value) => value,
        Err(message) => {
            field_errors.insert("external_source".to_owned(), serde_json::json!([message]));
            None
        }
    };
    let external_id = match opt_limited_string(issue_data, "external_id", 255) {
        Ok(value) => value,
        Err(message) => {
            field_errors.insert("external_id".to_owned(), serde_json::json!([message]));
            None
        }
    };
    // `blank=True` but no `null=True`: explicit null fails. The
    // `RegexValidator` allows only `[A-Za-z0-9._/-]*`.
    let git_work_branch = match issue_data.get("git_work_branch") {
        None => String::new(),
        Some(serde_json::Value::Null) => {
            field_errors.insert(
                "git_work_branch".to_owned(),
                serde_json::json!(["This field may not be null."]),
            );
            String::new()
        }
        Some(serde_json::Value::String(branch)) => {
            if branch.len() > 128 {
                field_errors.insert(
                    "git_work_branch".to_owned(),
                    serde_json::json!(["Ensure this field has no more than 128 characters."]),
                );
                String::new()
            } else if !branch
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'))
            {
                field_errors.insert(
                    "git_work_branch".to_owned(),
                    serde_json::json!([
                        "Branch name may contain only letters, numbers, and . _ / -"
                    ]),
                );
                String::new()
            } else {
                branch.clone()
            }
        }
        Some(serde_json::Value::Number(number)) => number.to_string(),
        Some(_) => {
            field_errors.insert(
                "git_work_branch".to_owned(),
                serde_json::json!(["Not a valid string."]),
            );
            String::new()
        }
    };
    // `description_stripped` input is validated like any `CharField`
    // but `Issue.save` recomputes it unconditionally, so the value is
    // validated and discarded.
    if let Some(value) = issue_data.get("description_stripped") {
        if char_internal(value).is_err() {
            let message = if value.is_null() {
                "This field may not be null."
            } else {
                "Not a valid string."
            };
            field_errors.insert(
                "description_stripped".to_owned(),
                serde_json::json!([message]),
            );
        }
    }
    let created_via = match opt_limited_string(issue_data, "created_via", 32) {
        Ok(value) => value,
        Err(message) => {
            field_errors.insert("created_via".to_owned(), serde_json::json!([message]));
            None
        }
    };
    let agent_executor = match issue_data.get("agent_executor") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(executor))
            if ["local_runner", "cloud_agent", "managed_runner"].contains(&executor.as_str()) =>
        {
            Some(executor.clone())
        }
        Some(serde_json::Value::String(executor)) => {
            field_errors.insert(
                "agent_executor".to_owned(),
                serde_json::json!([format!("\"{executor}\" is not a valid choice.")]),
            );
            None
        }
        Some(_) => {
            field_errors.insert(
                "agent_executor".to_owned(),
                serde_json::json!(["Not a valid string."]),
            );
            None
        }
    };
    let estimate_point_id =
        match opt_pk_field(issue_data, "estimate_point", pool, "estimate_points", true).await {
            Ok(id) => id,
            Err(message) => {
                field_errors.insert("estimate_point".to_owned(), serde_json::json!([message]));
                None
            }
        };
    let type_id = match opt_pk_field(issue_data, "type", pool, "issue_types", true).await {
        Ok(id) => id,
        Err(message) => {
            field_errors.insert("type".to_owned(), serde_json::json!([message]));
            None
        }
    };
    let deleted_at = match opt_datetime_field(issue_data, "deleted_at") {
        Ok(value) => value,
        Err(message) => {
            field_errors.insert("deleted_at".to_owned(), serde_json::json!([message]));
            None
        }
    };
    if !field_errors.is_empty() {
        // Field errors skip `validate()` entirely (DRF order).
        return Err(Denial::BadJson(serde_json::Value::Object(field_errors)).into_response());
    }
    // `validate()` (`serializers/issue.py:206-385`) in code order. The
    // sync lock needs an instance (create has none); binary validation
    // is dead (`description_binary` is read-only).
    if let (Some(start), Some(target)) = (start_date, target_date) {
        if start > target {
            return Err(Denial::BadJson(serde_json::json!({
                "non_field_errors": ["Start date cannot exceed target date"]
            }))
            .into_response());
        }
    }
    if let Some(pod_id) = assigned_pod_id {
        check_assigned_pod(pool, gate.project_id, pod_id).await?;
    }
    // Executor availability (`validate`, `:276-310`): the enum check
    // above is field-level; these are the `validate()` gates.
    if let Some(executor) = agent_executor.as_deref() {
        check_agent_executor(pool, settings, gate.user_id, executor).await?;
    }
    let description_html = match description_html {
        Some(html) if !html.is_empty() => match crate::space::sanitize::sanitize_html(&html) {
            crate::space::sanitize::Sanitize::Clean(clean) => Some(clean),
            crate::space::sanitize::Sanitize::Invalid => {
                return Err(Denial::BadJson(serde_json::json!({
                    "error": "html content is not valid"
                }))
                .into_response());
            }
        },
        other => other,
    };
    // Assignees keep active project members (`role__gte=15`); labels
    // keep project labels (`validate`, `:337-354`).
    let assignee_ids = match assignee_ids {
        Some(ids) if !ids.is_empty() => {
            Some(filter_project_assignees(pool, gate.project_id, &ids).await?)
        }
        _ => None,
    };
    let label_ids = match label_ids {
        Some(ids) if !ids.is_empty() => {
            Some(filter_project_labels(pool, gate.project_id, &ids).await?)
        }
        _ => None,
    };
    // `state` is the server-set triage row: the project check always
    // passes on this path (`allow_triage_state` context).
    let _ = triage_id;
    if let Some(parent) = parent_id {
        let in_project: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM \"issues\" WHERE (\"issues\".\"project_id\" = $1 AND \"issues\".\"id\" = $2 AND \"issues\".\"deleted_at\" IS NULL))",
        )
        .bind(gate.project_id)
        .bind(parent)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError.into_response())?;
        if !in_project {
            return Err(Denial::BadJson(serde_json::json!({
                "non_field_errors": ["Parent is not valid issue_id please pass a valid issue_id"]
            }))
            .into_response());
        }
    }
    if let Some(estimate) = estimate_point_id {
        let in_project: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM \"estimate_points\" WHERE (\"estimate_points\".\"project_id\" = $1 AND \"estimate_points\".\"id\" = $2 AND \"estimate_points\".\"deleted_at\" IS NULL))",
        )
        .bind(gate.project_id)
        .bind(estimate)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError.into_response())?;
        if !in_project {
            return Err(Denial::BadJson(serde_json::json!({
                "non_field_errors": ["Estimate point is not valid please pass a valid estimate_point_id"]
            }))
            .into_response());
        }
    }
    Ok(ValidatedCreate {
        parent_id,
        assigned_pod_id,
        label_ids,
        assignee_ids,
        point,
        name,
        description_json,
        description_html,
        priority,
        complexity_score,
        start_date,
        target_date,
        sequence_id,
        sort_order,
        completed_at,
        archived_at,
        is_draft,
        external_source,
        external_id,
        git_work_branch,
        created_via,
        agent_executor,
        estimate_point_id,
        type_id,
        deleted_at,
    })
}

// ---------------------------------------------------------------------------
// field coercions (DRF to_internal_value, in serializer field order)
// ---------------------------------------------------------------------------

/// `PrimaryKeyRelatedField.to_internal_value` (`relations.py:252-263`)
/// with a UUID `pk_field`: bools fail `incorrect_type` up front; strings
/// parse as UUIDs (curly-quote `not a valid UUID` message); ints go
/// straight to the lookup (`does_not_exist` — no UUID row ever matches);
/// floats/dicts/lists fail with the UUID message over their Python
/// `str()` (`5.5`, `{}`, `[]`).
#[allow(clippy::result_large_err)]
async fn resolve_pk(
    pool: &PgPool,
    table: &str,
    raw: &serde_json::Value,
    live_only: bool,
) -> Result<Uuid, String> {
    // Ints coerce through `UUID(int=n)` (`UUIDField.get_prep_value`):
    // out-of-range negatives fail `incorrect_type`, the rest look up a
    // (practically never matching) UUID while the message keeps the
    // original digits.
    enum Key {
        Uuid(Uuid),
        Int(Uuid, String),
    }
    let key = match raw {
        serde_json::Value::Bool(_) => {
            return Err("Incorrect type. Expected pk value, received bool.".to_owned());
        }
        serde_json::Value::String(text) => match text.parse::<Uuid>() {
            Ok(id) => Key::Uuid(id),
            Err(_) => return Err(uuid_invalid(text)),
        },
        serde_json::Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return Err("Incorrect type. Expected pk value, received int.".to_owned());
                }
                Key::Int(Uuid::from_u128(int as u128), int.to_string())
            } else if let Some(int) = number.as_u64() {
                Key::Int(Uuid::from_u128(u128::from(int)), int.to_string())
            } else if let Some(float) = number.as_f64() {
                return Err(uuid_invalid(&crate::paginator::py_float_str(float)));
            } else {
                return Err("Incorrect type. Expected pk value, received float.".to_owned());
            }
        }
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            return Err(uuid_invalid(&python_str_repr(raw)));
        }
        serde_json::Value::Null => {
            return Err("Incorrect type. Expected pk value, received NoneType.".to_owned());
        }
    };
    // Same-table parameterization keeps one helper. `live_only` mirrors
    // the field queryset: the default soft-delete-scoped managers
    // (`Issue.objects`, `Label.objects`, `EstimatePoint.objects`,
    // `IssueType.objects`) hide `deleted_at` rows, while
    // `Pod.all_objects` and the `users` table (no `deleted_at` column)
    // stay unfiltered.
    let (id, display) = match &key {
        Key::Uuid(id) => (*id, id.to_string()),
        Key::Int(id, text) => (*id, text.clone()),
    };
    let guard = if live_only {
        " AND (\"deleted_at\" IS NULL)"
    } else {
        ""
    };
    let exists: bool = sqlx::query_scalar(&format!(
        "SELECT EXISTS (SELECT 1 FROM \"{table}\" WHERE (\"id\" = $1){guard})"
    ))
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(|_| "Incorrect type. Expected pk value, received str.".to_owned())?;
    if !exists {
        return Err(format!("Invalid pk \"{display}\" - object does not exist."));
    }
    Ok(id)
}

/// DRF `UUIDField` failure message: curly quotes around the input.
fn uuid_invalid(input: &str) -> String {
    format!("\u{201c}{input}\u{201d} is not a valid UUID.")
}

/// Python `str()` for JSON values (DRF error-message interpolation):
/// single-quoted strings, `True`/`False`/`None`, Python float repr.
fn python_str_repr(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "None".to_owned(),
        serde_json::Value::Bool(true) => "True".to_owned(),
        serde_json::Value::Bool(false) => "False".to_owned(),
        serde_json::Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int.to_string()
            } else if let Some(int) = number.as_u64() {
                int.to_string()
            } else if let Some(float) = number.as_f64() {
                crate::paginator::py_float_str(float)
            } else {
                number.to_string()
            }
        }
        serde_json::Value::String(text) => format!("'{text}'"),
        serde_json::Value::Array(items) => {
            let inner = items
                .iter()
                .map(python_str_repr)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{inner}]")
        }
        serde_json::Value::Object(map) => {
            let inner = map
                .iter()
                .map(|(key, item)| format!("'{key}': {}", python_str_repr(item)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
    }
}

/// Optional `PrimaryKeyRelatedField`: missing/null → `None`; otherwise
/// [`resolve_pk`].
#[allow(clippy::result_large_err)]
async fn opt_pk_field(
    issue_data: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    pool: &PgPool,
    table: &str,
    live_only: bool,
) -> Result<Option<Uuid>, String> {
    let raw = match issue_data.get(key) {
        None | Some(serde_json::Value::Null) => return Ok(None),
        Some(raw) => raw,
    };
    resolve_pk(pool, table, raw, live_only).await.map(Some)
}

/// Optional `ListField(child=PrimaryKeyRelatedField)`: missing → `None`
/// (absent, not empty); non-list → `not_a_list`; per-item errors keyed
/// by index, exactly like DRF's `run_child_validation`.
#[allow(clippy::result_large_err)]
async fn opt_uuid_list_field(
    issue_data: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    pool: &PgPool,
    table: &str,
    live_only: bool,
) -> Result<Option<Vec<Uuid>>, serde_json::Value> {
    let raw = match issue_data.get(key) {
        None => return Ok(None),
        // `ListField` without `null=True`: explicit null fails.
        Some(serde_json::Value::Null) => {
            return Err(serde_json::json!(["This field may not be null."]));
        }
        Some(raw) => raw,
    };
    let serde_json::Value::Array(items) = raw else {
        return Err(serde_json::json!([format!(
            "Expected a list of items but got type \"{}\".",
            json_type_name(raw)
        )]));
    };
    let mut out = Vec::with_capacity(items.len());
    let mut errors = serde_json::Map::new();
    for (idx, item) in items.iter().enumerate() {
        match resolve_pk(pool, table, item, live_only).await {
            Ok(id) => out.push(id),
            Err(message) => {
                errors.insert(idx.to_string(), serde_json::json!([message]));
            }
        }
    }
    if !errors.is_empty() {
        return Err(serde_json::Value::Object(errors));
    }
    Ok(Some(out))
}

/// Python `type(x).__name__` for JSON values (DRF `incorrect_type` /
/// `not_a_list` messages).
fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "NoneType",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                "int"
            } else {
                "float"
            }
        }
        serde_json::Value::String(_) => "str",
        serde_json::Value::Array(_) => "list",
        serde_json::Value::Object(_) => "dict",
    }
}

/// Optional `IntegerField`: missing/null → `None`; bools fail; floats
/// fail; numeric strings coerce (DRF semantics).
fn opt_int_field(
    issue_data: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<i32>, String> {
    match issue_data.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(number)) => {
            if let Some(int) = number.as_i64() {
                i32::try_from(int)
                    .map(Some)
                    .map_err(|_| "A valid integer is required.".to_owned())
            } else {
                Err("A valid integer is required.".to_owned())
            }
        }
        Some(serde_json::Value::String(text)) => match text.trim().parse::<i64>() {
            Ok(int) => i32::try_from(int)
                .map(Some)
                .map_err(|_| "A valid integer is required.".to_owned()),
            Err(_) => Err("A valid integer is required.".to_owned()),
        },
        Some(_) => Err("A valid integer is required.".to_owned()),
    }
}

/// Optional `FloatField`: missing/null → `None`.
fn opt_float_field(
    issue_data: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<f64>, String> {
    match issue_data.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(number)) => number
            .as_f64()
            .map(Some)
            .ok_or_else(|| "A valid number is required.".to_owned()),
        Some(serde_json::Value::String(text)) => text
            .trim()
            .parse::<f64>()
            .map(Some)
            .map_err(|_| "A valid number is required.".to_owned()),
        Some(_) => Err("A valid number is required.".to_owned()),
    }
}

/// Optional `DateField` (`null=True`): missing/null → `None`.
fn opt_date_field(
    issue_data: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<chrono::NaiveDate>, String> {
    const MESSAGE: &str = "Date has wrong format. Use one of these formats instead: YYYY-MM-DD.";
    match issue_data.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(text)) => {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d") {
                return Ok(Some(date));
            }
            if let Some(moment) = parse_datetime_param(text) {
                return Ok(Some(moment.date_naive()));
            }
            Err(MESSAGE.to_owned())
        }
        Some(_) => Err(MESSAGE.to_owned()),
    }
}

/// Optional `DateTimeField` (`null=True`): missing/null → `None`; naive
/// values attach UTC like `DateTimeField.get_prep_value`.
fn opt_datetime_field(
    issue_data: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<DateTime<Utc>>, String> {
    const MESSAGE: &str = "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";
    match issue_data.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(text)) => parse_datetime_param(text)
            .map(Some)
            .ok_or_else(|| MESSAGE.to_owned()),
        Some(_) => Err(MESSAGE.to_owned()),
    }
}

/// Optional `BooleanField`: missing/null → `None` (null allowed on
/// `is_draft`? No — `is_draft` has no `null=True`, so null fails).
/// Kept generic: null → `None` only where the caller allows it.
fn opt_bool_field(
    issue_data: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<bool>, String> {
    const MESSAGE: &str = "Must be a valid boolean.";
    match issue_data.get(key) {
        None => Ok(None),
        Some(serde_json::Value::Bool(flag)) => Ok(Some(*flag)),
        Some(serde_json::Value::Number(number)) => {
            if number.as_i64() == Some(1) || number.as_u64() == Some(1) {
                Ok(Some(true))
            } else if number.as_i64() == Some(0) || number.as_u64() == Some(0) {
                Ok(Some(false))
            } else {
                Err(MESSAGE.to_owned())
            }
        }
        Some(serde_json::Value::String(text)) => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "t" | "yes" | "y" | "on" | "1" => Ok(Some(true)),
            "false" | "f" | "no" | "n" | "off" | "0" => Ok(Some(false)),
            _ => Err(MESSAGE.to_owned()),
        },
        Some(_) => Err(MESSAGE.to_owned()),
    }
}

/// Optional limited `CharField` (`null=True, blank=True`): missing/null
/// → `None`; trims; over-length → max_length error.
fn opt_limited_string(
    issue_data: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    max_length: usize,
) -> Result<Option<String>, String> {
    match issue_data.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(text)) => {
            let trimmed = text.trim().to_owned();
            if trimmed.chars().count() > max_length {
                return Err(format!(
                    "Ensure this field has no more than {max_length} characters."
                ));
            }
            Ok(Some(trimmed))
        }
        Some(serde_json::Value::Number(number)) => {
            let text = number.to_string();
            if text.chars().count() > max_length {
                return Err(format!(
                    "Ensure this field has no more than {max_length} characters."
                ));
            }
            Ok(Some(text))
        }
        Some(_) => Err("Not a valid string.".to_owned()),
    }
}

/// Executor availability (`validate`, `serializers/issue.py:276-310`).
///
/// * `local_runner` always dispatches.
/// * `cloud_agent` needs the instance kill switch
///   (`CLOUD_AGENT_ENABLED`); the per-run BYOK question is answered at
///   run creation, not here.
/// * `managed_runner` evaluates `managed_runner_availability` in gate
///   order; the desktop-unenrolled case (`NO_RUNNER_FOR_PROJECT`) is
///   ACCEPTED (the client self-resolves on open). The deeper gates
///   (LLM profile, enrollment, online presence) need the managed-runner
///   tables and are accepted once the kill switch + viewer gates pass —
///   verified only with the switch off (all contract/test envs); an
///   enabled instance with a viewer missing those rows would 400 in
///   Django where this stores. Boundary documented in the PR.
#[allow(clippy::result_large_err)]
async fn check_agent_executor(
    pool: &PgPool,
    settings: &pidash_db::config::Settings,
    user_id: Uuid,
    executor: &str,
) -> Result<(), Response> {
    if executor == "local_runner" {
        return Ok(());
    }
    if executor == "cloud_agent" {
        if !settings.cloud_agent.enabled {
            return Err(Denial::BadJson(serde_json::json!({
                "agent_executor": ["Pi Dash Cloud Agent is not available on this instance"]
            }))
            .into_response());
        }
        return Ok(());
    }
    debug_assert_eq!(executor, "managed_runner");
    if !settings.managed_runner.enabled {
        return Err(Denial::BadJson(serde_json::json!({
            "agent_executor": ["Pi Dash Agent is not enabled on this instance"]
        }))
        .into_response());
    }
    let viewer: Option<(bool, bool)> =
        sqlx::query_as("SELECT \"is_active\", \"is_bot\" FROM \"users\" WHERE (\"id\" = $1)")
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError.into_response())?;
    match viewer {
        Some((true, false)) => Ok(()),
        _ => Err(Denial::BadJson(serde_json::json!({
            "agent_executor": ["Open the Pi Dash desktop app to run on this computer"]
        }))
        .into_response()),
    }
}

/// `assigned_pod` project-equality + soft-delete checks (`validate`,
/// `serializers/issue.py:244-269`). The mid-flight rule needs an
/// instance (create has none). Errors are field errors.
#[allow(clippy::result_large_err)]
async fn check_assigned_pod(pool: &PgPool, project_id: Uuid, pod_id: Uuid) -> Result<(), Response> {
    let row: Option<(Uuid, Option<chrono::DateTime<Utc>>)> =
        sqlx::query_as("SELECT \"project_id\", \"deleted_at\" FROM \"pod\" WHERE (\"id\" = $1)")
            .bind(pod_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError.into_response())?;
    // The field check above already ensured existence; a concurrent
    // delete degrades to the friendly message, not the generic one.
    let (pod_project, deleted_at) = match row {
        Some(row) => row,
        None => {
            return Err(Denial::BadJson(serde_json::json!({
                "assigned_pod_id": ["pod has been deleted"]
            }))
            .into_response());
        }
    };
    if pod_project != project_id {
        return Err(Denial::BadJson(serde_json::json!({
            "assigned_pod_id": ["pod is in a different project"]
        }))
        .into_response());
    }
    if deleted_at.is_some() {
        return Err(Denial::BadJson(serde_json::json!({
            "assigned_pod_id": ["pod has been deleted"]
        }))
        .into_response());
    }
    Ok(())
}

/// Keep assignees that are active project members (`role__gte=15`).
#[allow(clippy::result_large_err)]
async fn filter_project_assignees(
    pool: &PgPool,
    project_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, Response> {
    let list = ids
        .iter()
        .map(|id| format!("'{id}'"))
        .collect::<Vec<_>>()
        .join(",");
    // `ProjectMember.objects` is the soft-delete-scoped default manager.
    let rows: Vec<Uuid> = sqlx::query_scalar(&format!(
        "SELECT \"member_id\" FROM \"project_members\" WHERE (\"project_id\" = $1 AND \"role\" >= 15 AND \"is_active\" AND \"deleted_at\" IS NULL AND \"member_id\" IN ({list}))"
    ))
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    Ok(rows)
}

/// Keep labels that belong to the project.
#[allow(clippy::result_large_err)]
async fn filter_project_labels(
    pool: &PgPool,
    project_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, Response> {
    let list = ids
        .iter()
        .map(|id| format!("'{id}'"))
        .collect::<Vec<_>>()
        .join(",");
    // `Label.objects` is the soft-delete-scoped default manager.
    let rows: Vec<Uuid> = sqlx::query_scalar(&format!(
        "SELECT \"id\" FROM \"labels\" WHERE (\"project_id\" = $1 AND \"deleted_at\" IS NULL AND \"id\" IN ({list}))"
    ))
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// create: Issue + m2m writes
// ---------------------------------------------------------------------------

/// The `issues` row of `IssueCreateSerializer.create`
/// (`serializers/issue.py:305`) with `Issue.save`
/// (`db/models/issue.py:267-351`) and `BaseModel.save` (crum
/// `created_by`, `db/models/base.py:24-46`), written on the caller's
/// sequence transaction. Sort-order and pod resolution stay with the
/// caller: the EXISTS check runs under the advisory lock like Django's
/// MAX read, the pod lookup is a stable read outside the atomic block.
/// The assignee/label links are [`insert_issue_m2m`], after the commit.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::result_large_err)]
async fn insert_issue(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    gate: &crate::app_issues::Gate,
    validated: &ValidatedCreate,
    issue_id: Uuid,
    triage_id: Uuid,
    sequence_id: i32,
    sort_order: f64,
    now: DateTime<Utc>,
    assigned_pod_id: Option<Uuid>,
) -> Result<(), Response> {
    // `save`: `None` only when the html itself is `""`/`None`;
    // otherwise the stripped text, even when it strips to `""`.
    // The stripper is `utils/html_processor.py` (MLStripper, entities
    // decoded), not Django's verbatim `strip_tags`.
    let description_stripped = match validated.description_html.as_deref() {
        None | Some("") => None,
        Some(html) => Some(ml_strip_tags(html)),
    };
    if let Err(error) = sqlx::query(
        "INSERT INTO \"issues\" (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"project_id\", \"workspace_id\", \"parent_id\", \"state_id\", \"point\", \"estimate_point_id\", \"name\", \"description_json\", \"description_html\", \"description_stripped\", \"description_binary\", \"priority\", \"complexity_score\", \"start_date\", \"target_date\", \"sequence_id\", \"sort_order\", \"completed_at\", \"archived_at\", \"is_draft\", \"external_source\", \"external_id\", \"type_id\", \"git_work_branch\", \"workpad\", \"created_via\", \"assigned_pod_id\", \"agent_executor\") VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, NULL, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, '', $29, $30, $31)",
    )
    .bind(issue_id)
    .bind(now)
    .bind(now)
    .bind(gate.user_id)
    .bind(validated.deleted_at)
    .bind(gate.project_id)
    .bind(gate.workspace_id)
    .bind(validated.parent_id)
    .bind(triage_id)
    .bind(validated.point)
    .bind(validated.estimate_point_id)
    .bind(validated.name.clone())
    .bind(validated.description_json.clone())
    .bind(validated.description_html.clone())
    .bind(description_stripped)
    .bind(validated.priority.clone())
    .bind(validated.complexity_score)
    .bind(validated.start_date)
    .bind(validated.target_date)
    .bind(sequence_id)
    .bind(sort_order)
    // `Issue.save` clears `completed_at` for non-completed states; the
    // state here is always triage, so the stored value is always NULL.
    .bind(None::<DateTime<Utc>>)
    .bind(validated.archived_at)
    .bind(validated.is_draft)
    .bind(validated.external_source.clone())
    .bind(validated.external_id.clone())
    .bind(validated.type_id)
    .bind(validated.git_work_branch.clone())
    .bind(validated.created_via.clone())
    .bind(assigned_pod_id)
    .bind(validated.agent_executor.clone())
    .execute(&mut **tx)
    .await
    {
        return Err(db_error(error));
    }
    Ok(())
}

/// Assignees + labels (`create`, `:311-369`): validated ids, else the
/// project default when it is an active member (`role__gte=15`).
/// Integrity failures are swallowed like Django's `except IntegrityError:
/// pass`. Runs after the caller's sequence transaction commits — Django
/// writes these after `Issue.objects.create` returns.
#[allow(clippy::result_large_err)]
async fn insert_issue_m2m(
    pool: &PgPool,
    gate: &crate::app_issues::Gate,
    validated: &ValidatedCreate,
    issue_id: Uuid,
    now: DateTime<Utc>,
    default_assignee_id: Option<Uuid>,
) -> Result<(), Response> {
    let assignees = match &validated.assignee_ids {
        Some(ids) => ids.clone(),
        None => match default_assignee_id {
            Some(default) => {
                let valid: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM \"project_members\" WHERE (\"member_id\" = $1 AND \"project_id\" = $2 AND \"role\" >= 15 AND \"is_active\"))",
                )
                .bind(default)
                .bind(gate.project_id)
                .fetch_one(pool)
                .await
                .map_err(|_| Denial::ServerError.into_response())?;
                if valid {
                    vec![default]
                } else {
                    Vec::new()
                }
            }
            None => Vec::new(),
        },
    };
    for assignee_id in &assignees {
        let _ = sqlx::query(
            "INSERT INTO \"issue_assignees\" (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"project_id\", \"workspace_id\", \"issue_id\", \"assignee_id\") VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8) ON CONFLICT DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(now)
        .bind(now)
        .bind(gate.user_id)
        .bind(gate.project_id)
        .bind(gate.workspace_id)
        .bind(issue_id)
        .bind(*assignee_id)
        .execute(pool)
        .await;
    }
    if let Some(label_ids) = &validated.label_ids {
        for label_id in label_ids {
            let _ = sqlx::query(
                "INSERT INTO \"issue_labels\" (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"project_id\", \"workspace_id\", \"issue_id\", \"label_id\") VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8) ON CONFLICT DO NOTHING",
            )
            .bind(Uuid::new_v4())
            .bind(now)
            .bind(now)
            .bind(gate.user_id)
            .bind(gate.project_id)
            .bind(gate.workspace_id)
            .bind(issue_id)
            .bind(*label_id)
            .execute(pool)
            .await;
        }
    }
    Ok(())
}

/// The project's default pod (`Pod.default_for_project_id`), if any.
#[allow(clippy::result_large_err)]
async fn default_project_pod(pool: &PgPool, project_id: Uuid) -> Result<Option<Uuid>, Response> {
    sqlx::query_scalar(
        "SELECT \"id\" FROM \"pod\" WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"project_id\" = $1 AND \"pod\".\"is_default\" = TRUE) ORDER BY \"pod\".\"created_at\" ASC LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map(|row| row.flatten())
    .map_err(|_| Denial::ServerError.into_response())
}

// ---------------------------------------------------------------------------
// create: re-fetch + IntakeIssueDetailSerializer render
// ---------------------------------------------------------------------------

/// One re-fetched bridge row.
type BridgeRow = (
    Uuid,
    i32,
    Option<Uuid>,
    Option<DateTime<Utc>>,
    Option<String>,
);

/// The re-fetched bridge + issue columns (`:293-322`).
#[allow(clippy::too_many_arguments)]
struct CreatedRows {
    bridge_id: Uuid,
    status: i32,
    duplicate_to_id: Option<Uuid>,
    snoozed_till: Option<DateTime<Utc>>,
    source: Option<String>,
    issue_id: Uuid,
    name: String,
    state_id: Option<Uuid>,
    sort_order: f64,
    estimate_point_id: Option<Uuid>,
    priority: String,
    complexity_score: i32,
    start_date: Option<chrono::NaiveDate>,
    target_date: Option<chrono::NaiveDate>,
    sequence_id: i32,
    project_id: Uuid,
    parent_id: Option<Uuid>,
    assigned_pod_id: Option<Uuid>,
    agent_executor: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    is_draft: bool,
    archived_at: Option<chrono::NaiveDate>,
    description_html: Option<String>,
    label_ids: Vec<Uuid>,
    assignee_ids: Vec<Uuid>,
}

/// Re-fetch (`:293-322`) and render `IntakeIssueDetailSerializer`
/// (`:323-324`, 200).
///
/// Four narrow queries stand in for the one Django queryset (bridge +
/// issue + the two annotates): identical rows without the `t.*`/`i.*`
/// column collisions. The label annotate keeps the through-table
/// deleted guard; the assignee annotate keeps `is_active` WITHOUT the
/// through-table deleted guard — the recorded asymmetry
/// (`base.py:307-315`).
#[allow(clippy::result_large_err)]
async fn render_created_detail(
    pool: &PgPool,
    gate: &crate::app_issues::Gate,
    intake_id: Uuid,
    issue_id: Uuid,
    _bridge_id: Uuid,
) -> Result<String, Response> {
    let bridge: Option<BridgeRow> = sqlx::query_as(
        "SELECT \"id\", \"status\", \"duplicate_to_id\", \"snoozed_till\", \"source\" FROM \"intake_issues\" WHERE (\"intake_id\" = $1 AND \"issue_id\" = $2 AND \"project_id\" = $3 AND \"deleted_at\" IS NULL)",
    )
    .bind(intake_id)
    .bind(issue_id)
    .bind(gate.project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    let Some((bridge_id, status, duplicate_to_id, snoozed_till, source)) = bridge else {
        return Err(Denial::ServerError.into_response());
    };
    // Untyped fetch (a 21-column tuple exceeds sqlx's `FromRow`
    // impls); single-table so names are unambiguous.
    let issue = sqlx::query(
        "SELECT \"id\", \"name\", \"state_id\", \"sort_order\", \"estimate_point_id\", \"priority\", \"complexity_score\", \"start_date\", \"target_date\", \"sequence_id\", \"project_id\", \"parent_id\", \"assigned_pod_id\", \"agent_executor\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"is_draft\", \"archived_at\", \"description_html\" FROM \"issues\" WHERE (\"id\" = $1)",
    )
    .bind(issue_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    let label_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT COALESCE(ARRAY_AGG(DISTINCT \"labels\".\"id\") FILTER (WHERE (\"labels\".\"id\" IS NOT NULL AND \"issue_labels\".\"deleted_at\" IS NULL)), '{}') FROM \"issues\" LEFT OUTER JOIN \"issue_labels\" ON (\"issues\".\"id\" = \"issue_labels\".\"issue_id\") LEFT OUTER JOIN \"labels\" ON (\"issue_labels\".\"label_id\" = \"labels\".\"id\") WHERE (\"issues\".\"id\" = $1)",
    )
    .bind(issue_id)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    // `member_project` is the `User → ProjectMember` reverse join with
    // no project scoping, exactly as Django spans it.
    let assignee_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT COALESCE(ARRAY_AGG(DISTINCT \"users\".\"id\") FILTER (WHERE (\"users\".\"id\" IS NOT NULL AND \"member_project\".\"is_active\")), '{}') FROM \"issues\" LEFT OUTER JOIN \"issue_assignees\" ON (\"issues\".\"id\" = \"issue_assignees\".\"issue_id\") LEFT OUTER JOIN \"users\" ON (\"issue_assignees\".\"assignee_id\" = \"users\".\"id\") LEFT OUTER JOIN \"project_members\" \"member_project\" ON (\"users\".\"id\" = \"member_project\".\"member_id\") WHERE (\"issues\".\"id\" = $1)",
    )
    .bind(issue_id)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    let Some(fetched) = issue else {
        return Err(Denial::ServerError.into_response());
    };
    let get = |column: &str| fetched.try_get::<Uuid, _>(column);
    let rows = CreatedRows {
        bridge_id,
        status,
        duplicate_to_id,
        snoozed_till,
        source,
        issue_id: get("id").map_err(|_| Denial::ServerError.into_response())?,
        name: fetched
            .try_get::<String, _>("name")
            .map_err(|_| Denial::ServerError.into_response())?,
        state_id: fetched
            .try_get::<Option<Uuid>, _>("state_id")
            .map_err(|_| Denial::ServerError.into_response())?,
        sort_order: fetched
            .try_get::<f64, _>("sort_order")
            .map_err(|_| Denial::ServerError.into_response())?,
        estimate_point_id: fetched
            .try_get::<Option<Uuid>, _>("estimate_point_id")
            .map_err(|_| Denial::ServerError.into_response())?,
        priority: fetched
            .try_get::<String, _>("priority")
            .map_err(|_| Denial::ServerError.into_response())?,
        complexity_score: fetched
            .try_get::<i32, _>("complexity_score")
            .map_err(|_| Denial::ServerError.into_response())?,
        start_date: fetched
            .try_get::<Option<chrono::NaiveDate>, _>("start_date")
            .map_err(|_| Denial::ServerError.into_response())?,
        target_date: fetched
            .try_get::<Option<chrono::NaiveDate>, _>("target_date")
            .map_err(|_| Denial::ServerError.into_response())?,
        sequence_id: fetched
            .try_get::<i32, _>("sequence_id")
            .map_err(|_| Denial::ServerError.into_response())?,
        project_id: fetched
            .try_get::<Uuid, _>("project_id")
            .map_err(|_| Denial::ServerError.into_response())?,
        parent_id: fetched
            .try_get::<Option<Uuid>, _>("parent_id")
            .map_err(|_| Denial::ServerError.into_response())?,
        assigned_pod_id: fetched
            .try_get::<Option<Uuid>, _>("assigned_pod_id")
            .map_err(|_| Denial::ServerError.into_response())?,
        agent_executor: fetched
            .try_get::<Option<String>, _>("agent_executor")
            .map_err(|_| Denial::ServerError.into_response())?,
        created_at: fetched
            .try_get::<DateTime<Utc>, _>("created_at")
            .map_err(|_| Denial::ServerError.into_response())?,
        updated_at: fetched
            .try_get::<DateTime<Utc>, _>("updated_at")
            .map_err(|_| Denial::ServerError.into_response())?,
        created_by_id: fetched
            .try_get::<Option<Uuid>, _>("created_by_id")
            .map_err(|_| Denial::ServerError.into_response())?,
        updated_by_id: fetched
            .try_get::<Option<Uuid>, _>("updated_by_id")
            .map_err(|_| Denial::ServerError.into_response())?,
        is_draft: fetched
            .try_get::<bool, _>("is_draft")
            .map_err(|_| Denial::ServerError.into_response())?,
        archived_at: fetched
            .try_get::<Option<chrono::NaiveDate>, _>("archived_at")
            .map_err(|_| Denial::ServerError.into_response())?,
        description_html: fetched
            .try_get::<Option<String>, _>("description_html")
            .map_err(|_| Denial::ServerError.into_response())?,
        label_ids,
        assignee_ids,
    };
    let nested = render_created_issue(&rows, gate.timezone);
    let bridge_id_text = rows.bridge_id.to_string();
    let duplicate_to_text = rows.duplicate_to_id.map(|id| id.to_string());
    let snoozed_text = rows
        .snoozed_till
        .map(|moment| crate::serializer::render_datetime_in(&moment, &gate.timezone));
    let detail = pidash_services::app_intake::shape::render_detail(
        &pidash_services::app_intake::shape::DetailRow {
            id: &bridge_id_text,
            status: i64::from(rows.status),
            duplicate_to: duplicate_to_text.as_deref(),
            snoozed_till: snoozed_text.as_deref(),
            duplicate_issue_detail: None,
            source: rows.source.as_deref(),
            issue: nested,
        },
    );
    Ok(detail.to_string())
}

/// The nested `IssueDetailSerializer` for a just-created issue, in
/// `Meta.fields` order. Annotation-less read-only fields (`cycle_id`,
/// `module_ids`, the three counts) and attribute-less declared fields
/// (`is_subscribed`, `is_intake`) are absent, exactly as DRF skips
/// them; ticker/runs/relations are null/empty on a fresh row.
fn render_created_issue(rows: &CreatedRows, timezone: chrono_tz::Tz) -> serde_json::Value {
    let mut issue = serde_json::Map::with_capacity(32);
    issue.insert("id".to_owned(), uuid_string(&rows.issue_id));
    issue.insert(
        "name".to_owned(),
        serde_json::Value::String(rows.name.clone()),
    );
    issue.insert("state_id".to_owned(), opt_uuid(&rows.state_id));
    issue.insert(
        "sort_order".to_owned(),
        serde_json::Number::from_f64(rows.sort_order)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
    );
    // `save` clears `completed_at` whenever the state is set and not
    // completed — always the case here (triage).
    issue.insert("completed_at".to_owned(), serde_json::Value::Null);
    issue.insert(
        "estimate_point".to_owned(),
        opt_uuid(&rows.estimate_point_id),
    );
    issue.insert(
        "priority".to_owned(),
        serde_json::Value::String(rows.priority.clone()),
    );
    issue.insert(
        "complexity_score".to_owned(),
        serde_json::Value::from(rows.complexity_score),
    );
    issue.insert(
        "start_date".to_owned(),
        rows.start_date
            .map(|day| serde_json::Value::String(day.format("%Y-%m-%d").to_string()))
            .unwrap_or(serde_json::Value::Null),
    );
    issue.insert(
        "target_date".to_owned(),
        rows.target_date
            .map(|day| serde_json::Value::String(day.format("%Y-%m-%d").to_string()))
            .unwrap_or(serde_json::Value::Null),
    );
    issue.insert(
        "sequence_id".to_owned(),
        serde_json::Value::from(rows.sequence_id),
    );
    issue.insert("project_id".to_owned(), uuid_string(&rows.project_id));
    issue.insert("parent_id".to_owned(), opt_uuid(&rows.parent_id));
    issue.insert(
        "assigned_pod_id".to_owned(),
        opt_uuid(&rows.assigned_pod_id),
    );
    issue.insert(
        "agent_executor".to_owned(),
        opt_string(&rows.agent_executor),
    );
    issue.insert(
        "label_ids".to_owned(),
        serde_json::Value::Array(rows.label_ids.iter().map(uuid_string).collect()),
    );
    issue.insert(
        "assignee_ids".to_owned(),
        serde_json::Value::Array(rows.assignee_ids.iter().map(uuid_string).collect()),
    );
    issue.insert(
        "created_at".to_owned(),
        serde_json::Value::String(crate::serializer::render_datetime_in(
            &rows.created_at,
            &timezone,
        )),
    );
    issue.insert(
        "updated_at".to_owned(),
        serde_json::Value::String(crate::serializer::render_datetime_in(
            &rows.updated_at,
            &timezone,
        )),
    );
    issue.insert("created_by".to_owned(), opt_uuid(&rows.created_by_id));
    issue.insert("updated_by".to_owned(), opt_uuid(&rows.updated_by_id));
    issue.insert(
        "is_draft".to_owned(),
        serde_json::Value::Bool(rows.is_draft),
    );
    issue.insert(
        "archived_at".to_owned(),
        rows.archived_at
            .map(|day| serde_json::Value::String(day.format("%Y-%m-%d").to_string()))
            .unwrap_or(serde_json::Value::Null),
    );
    issue.insert("is_synced".to_owned(), serde_json::Value::Bool(false));
    issue.insert(
        "description_html".to_owned(),
        opt_string(&rows.description_html),
    );
    issue.insert("agent_ticker".to_owned(), serde_json::Value::Null);
    issue.insert("agent_status".to_owned(), serde_json::Value::Null);
    issue.insert(
        "relations_summary".to_owned(),
        serde_json::json!({"blocked_by": [], "blocking": []}),
    );
    issue.insert(
        "has_open_blockers".to_owned(),
        serde_json::Value::Bool(false),
    );
    serde_json::Value::Object(issue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
            let mut errs = FieldErrors::new();
            let parsed =
                validate_datetime(&mut errs, "completed_at", &json!(text), false).expect(text);
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
            let mut errs = FieldErrors::new();
            assert!(
                validate_datetime(&mut errs, "completed_at", &json!(text), false).is_none(),
                "{text:?}"
            );
        }
    }
}
