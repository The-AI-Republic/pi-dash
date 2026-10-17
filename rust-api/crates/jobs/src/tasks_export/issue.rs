//! `issue_export_task` handler (stage 8, PIDASHCONV-1106).
//!
//! Port of `apps/api/pi_dash/bgtasks/export_task.py:127-226`
//! (`issue_export_task`) plus `upload_to_s3` (`:20-125`). The Celery wire
//! name is [`super::ISSUE_EXPORT_TASK`]; the view's `.delay()` call
//! passes `provider`/`workspace_id`/`project_ids`/`token_id`/`multiple`/
//! `slug` as keywords (`app/views/exporter/base.py:49-56`).
//!
//! The body is a pipeline over shared kernels: the wire binds through
//! [`super::bind_export_task_call`], the status writes through the
//! `mark_*_sql` builders, rows serialize through
//! [`issue_export_to_representation`](pidash_services::app_analytics::shape::issue_export_to_representation)
//! and the porter engines (`porter_csv_encode` /
//! `porter_json_encode_indent` / `porter_xlsx_encode`), the zip through
//! [`build_zip`](pidash_services::tasks_cleanup::exports::build_zip),
//! and the S3 upload + presigned URL through `pidash_storage` (PUT via
//! [`sign_put`](pidash_storage::sign_put) /
//! [`sign_put_extra`](pidash_storage::sign_put_extra), URL via
//! [`presign_get_url`](pidash_storage::presign_get_url) — all verified
//! byte-for-byte against botocore vectors).
//!
//! Ack parity: the bare `@shared_task` returns normally on every handled
//! path (success, bad provider, swallowed exception), so the handler
//! settles those to [`Verdict::Ack`]. Two paths raise out of the task in
//! Python — an unbindable payload (`TypeError`) and a missing exporter
//! row (`DoesNotExist`, re-raised by the `except` block's own `.get()`)
//! — which Celery settles as terminal FAILURE with no redelivery: the
//! handler returns `Err` for the former (requeue-with-budget, then the
//! row parks) and [`Verdict::Fail`] for the latter (immediate park).
//!
//! Ported bugs (also listed in the PR): BUG-EXPORT-1 — the task queryset
//! carries no `Count` annotations, so DRF `SkipField`s the three count
//! columns (`sub_issues_count`, `link_count`, `attachment_count`) out of
//! every exported row; the handler omits the keys (see
//! [`EXPORT_ROW_FIELDS`]).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use pidash_services::app_analytics::export_format as porter;
use pidash_services::app_analytics::shape as export_shape;
use pidash_services::tasks_cleanup::exports as ex;
use pidash_types::tasks_cleanup::exports_dto as dto;
use sqlx::PgPool;
use uuid::Uuid;

use super::{
    bind_export_task_call, export_project_narrow_clause, fetch_exporter_by_token_sql,
    mark_failed_sql, mark_processing_sql, mark_uploaded_sql, task_export_filename,
    validate_task_provider, ExportTaskCall, ISSUE_EXPORT_TASK, STATUS_FAILED, STATUS_PROCESSING,
};
use crate::queue::JobRow;
use crate::worker::{Handler, Verdict};

/// Row keys in serializer `Meta.fields` order
/// (`porters/serializers/issue.py:95-124`) MINUS the three `Count`
/// annotations the task queryset never applies: DRF `SkipField`s
/// `sub_issues_count`, `link_count` and `attachment_count` (each is
/// `IntegerField(read_only=True)` on an unannotated queryset), so the
/// keys never appear in any exported row (BUG-EXPORT-1, ported).
pub const EXPORT_ROW_FIELDS: &[&str] = &[
    "project_name",
    "project_identifier",
    "parent",
    "identifier",
    "sequence_id",
    "name",
    "state_name",
    "priority",
    "complexity_score",
    "assignees",
    "subscribers",
    "created_by_name",
    "start_date",
    "target_date",
    "completed_at",
    "created_at",
    "updated_at",
    "archived_at",
    "estimate",
    "labels",
    "cycles",
    "modules",
    "links",
    "relations",
    "comments",
    "is_draft",
];

/// DRF `DateTimeField` output (`USE_TZ`, `TIME_ZONE="UTC"`, default
/// ISO-8601 format): `Z` suffix, microseconds iff nonzero, `None` stays
/// `None` (probed on DRF 3.15).
pub fn drf_datetime(value: Option<DateTime<Utc>>) -> Option<String> {
    value.map(|moment| {
        if moment.timestamp_subsec_micros() == 0 {
            moment.format("%Y-%m-%dT%H:%M:%SZ").to_string()
        } else {
            moment.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()
        }
    })
}

/// DRF `DateField` output: `YYYY-MM-DD`, `None` stays `None`.
pub fn drf_date(value: Option<NaiveDate>) -> Option<String> {
    value.map(|day| day.format("%Y-%m-%d").to_string())
}

/// `User.full_name` (`db/models/user.py:167`):
/// `f"{first} {last}".strip()` (`None` renders `"None"`, like the
/// f-string).
pub fn full_name(first_name: Option<&str>, last_name: Option<&str>) -> String {
    format!(
        "{} {}",
        first_name.unwrap_or("None"),
        last_name.unwrap_or("None")
    )
    .trim()
    .to_owned()
}

/// The S3 settings the upload branches read
/// (`settings/common.py:372-387`, all env-sourced with registry
/// defaults), resolved once per task run.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportS3 {
    pub use_minio: bool,
    pub endpoint_url: Option<String>,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub bucket: String,
    pub region_setting: String,
    pub web_url: String,
    pub default_region: Option<String>,
}

impl ExportS3 {
    fn env(key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|value| !value.is_empty())
    }

    /// Read the upload settings from the environment with Django's
    /// registry defaults. `AWS_DEFAULT_REGION` reads verbatim (even
    /// `""`, like botocore); shared-config files are NOT consulted
    /// (botocore would read them — the contract runs scrub them so both
    /// backends resolve `us-east-1`; see the suite docs).
    pub fn from_env() -> Self {
        let use_minio = Self::env("USE_MINIO")
            .and_then(|raw| raw.trim().parse::<i64>().ok())
            .is_some_and(|flag| flag == 1);
        let endpoint_url =
            Self::env("AWS_S3_ENDPOINT_URL").or_else(|| Self::env("MINIO_ENDPOINT_URL"));
        Self {
            use_minio,
            endpoint_url,
            access_key_id: Self::env("AWS_ACCESS_KEY_ID")
                .unwrap_or_else(|| "access-key".to_owned()),
            secret_access_key: Self::env("AWS_SECRET_ACCESS_KEY")
                .unwrap_or_else(|| "secret-key".to_owned()),
            bucket: Self::env("AWS_S3_BUCKET_NAME").unwrap_or_else(|| "uploads".to_owned()),
            region_setting: Self::env("AWS_REGION").unwrap_or_default(),
            web_url: Self::env("WEB_URL").unwrap_or_else(|| "http://localhost".to_owned()),
            default_region: std::env::var("AWS_DEFAULT_REGION").ok(),
        }
    }

    /// The SigV4 scope region for the MinIO/endpoint branches: Django
    /// passes no region there, so botocore resolves its own chain
    /// (`AWS_DEFAULT_REGION`, verbatim even when `""`, else the
    /// `us-east-1` default — `AWS_REGION` is NOT consulted).
    pub fn client_region(&self) -> String {
        self.default_region
            .clone()
            .unwrap_or_else(|| "us-east-1".to_owned())
    }
}

/// The three upload branches (`export_task.py:24-112`), in order.
#[derive(Debug, Clone, PartialEq)]
pub enum UploadBranch {
    Minio,
    Endpoint,
    Region,
}

pub fn upload_branch(s3: &ExportS3) -> UploadBranch {
    if s3.use_minio {
        UploadBranch::Minio
    } else if s3.endpoint_url.is_some() {
        UploadBranch::Endpoint
    } else {
        UploadBranch::Region
    }
}

/// `settings.AWS_S3_URL_PROTOCOL` / `settings.AWS_S3_CUSTOM_DOMAIN` for
/// the MinIO branch (`common.py:384-387`): derived from `WEB_URL`, and
/// defined ONLY when an endpoint URL is also set — otherwise the
/// attribute lookup raises `AttributeError` after the upload (ported as
/// an error here).
pub fn minio_presign_parts(s3: &ExportS3) -> Result<(String, String), String> {
    if s3.endpoint_url.is_none() {
        return Err("issue_export_task: MinIO presign endpoint missing \
             (Django raises AttributeError: no AWS_S3_CUSTOM_DOMAIN)"
            .to_owned());
    }
    let parsed = reqwest::Url::parse(&s3.web_url)
        .map_err(|_| "issue_export_task: bad WEB_URL".to_owned())?;
    let scheme = parsed.scheme().to_owned();
    let netloc = parsed
        .host_str()
        .map(|host| match parsed.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        })
        .unwrap_or_default();
    Ok((format!("{scheme}:"), format!("{netloc}/{}", s3.bucket)))
}

/// Split an endpoint URL into base + signed host for the presign signer
/// (no trailing slash on the base; the host keeps its port).
pub fn endpoint_parts(endpoint_url: &str) -> Result<(String, String), String> {
    let parsed = reqwest::Url::parse(endpoint_url)
        .map_err(|_| "issue_export_task: bad endpoint URL".to_owned())?;
    let host = parsed
        .host_str()
        .map(|host| match parsed.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        })
        .ok_or_else(|| "issue_export_task: bad endpoint URL".to_owned())?;
    Ok((endpoint_url.trim_end_matches('/').to_owned(), host))
}

/// The region-branch base endpoint (`https://{bucket}.s3[.{region}].
/// amazonaws.com`, virtual-host; `us-east-1` keeps the legacy global
/// endpoint — probed botocore behavior).
pub fn region_base(bucket: &str, region: &str) -> (String, String) {
    let host = if region == "us-east-1" {
        format!("{bucket}.s3.amazonaws.com")
    } else {
        format!("{bucket}.s3.{region}.amazonaws.com")
    };
    (format!("https://{host}"), host)
}

/// Coerce one wire UUID (`UUIDField.get_prep_value`): garbage raises
/// `ValidationError(["'{v}' is not a valid UUID."])`, whose `str()` is
/// the task's `reason` verbatim. Filters validate in call order
/// (workspace, then `project_id__in` item by item, then member), so the
/// first bad value wins.
pub fn wire_uuid(value: &str) -> Result<Uuid, String> {
    Uuid::parse_str(value).map_err(|_| format!("[\"'{value}' is not a valid UUID.\"]"))
}

/// The task queryset's select columns: the issue row plus the
/// `select_related` names (`export_task.py:156-162`).
#[derive(Debug, Clone)]
pub struct IssueRow {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub project_id: Uuid,
    pub project_name: String,
    pub project_identifier: String,
    pub sequence_id: i32,
    pub name: String,
    pub state_name: Option<String>,
    pub priority: String,
    pub complexity_score: i32,
    pub estimate_value: Option<String>,
    pub created_first: Option<String>,
    pub created_last: Option<String>,
    pub has_creator: bool,
    pub start_date: Option<NaiveDate>,
    pub target_date: Option<NaiveDate>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<NaiveDate>,
    pub is_draft: bool,
}

/// The full issue statement: scope joins + [`super::export_issues_where_sql`]
/// plus the `select_related` legs and default `-created_at` ordering
/// (`db/models/issue.py:254`). `extra` appends the per-project narrow
/// clause for the `multiple` loop.
pub fn export_issues_sql(
    workspace_ph: &str,
    project_phs: &[String],
    member_ph: &str,
    extra: Option<&str>,
) -> String {
    let mut where_sql = super::export_issues_where_sql(workspace_ph, project_phs, member_ph);
    if let Some(narrow) = extra {
        where_sql.push_str(&format!(" AND ({narrow})"));
    }
    format!(
        "SELECT \"issues\".\"id\", \"issues\".\"parent_id\", \"issues\".\"project_id\", \
         \"projects\".\"name\" AS \"project_name\", \
         \"projects\".\"identifier\" AS \"project_identifier\", \
         \"issues\".\"sequence_id\", \"issues\".\"name\", \
         \"states\".\"name\" AS \"state_name\", \"issues\".\"priority\", \
         \"issues\".\"complexity_score\", \
         \"estimate_points\".\"value\" AS \"estimate_value\", \
         \"creators\".\"first_name\" AS \"created_first\", \
         \"creators\".\"last_name\" AS \"created_last\", \
         \"issues\".\"created_by_id\" AS \"creator_id\", \
         \"issues\".\"start_date\", \"issues\".\"target_date\", \
         \"issues\".\"completed_at\", \"issues\".\"created_at\", \
         \"issues\".\"updated_at\", \"issues\".\"archived_at\", \
         \"issues\".\"is_draft\" \
         FROM \"issues\" \
         INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") \
         LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") \
         INNER JOIN \"project_members\" ON (\"projects\".\"id\" = \"project_members\".\"project_id\") \
         LEFT OUTER JOIN \"users\" AS \"creators\" \
         ON (\"issues\".\"created_by_id\" = \"creators\".\"id\") \
         LEFT OUTER JOIN \"estimate_points\" \
         ON (\"issues\".\"estimate_point_id\" = \"estimate_points\".\"id\") \
         WHERE {where_sql} ORDER BY \"issues\".\"created_at\" DESC"
    )
}

async fn fetch_issues(
    pool: &PgPool,
    workspace: Uuid,
    projects: &[Uuid],
    member: Uuid,
    narrow: Option<Uuid>,
) -> Result<Vec<IssueRow>, String> {
    if projects.is_empty() {
        // `filter(project_id__in=[])` is Django's `EmptyResultSet`: no
        // query runs, zero rows come back (never `IN ()`).
        return Ok(Vec::new());
    }
    // Placeholders: $1 workspace, $2.. project ids, member, narrow.
    let mut index = 1;
    let workspace_ph = format!("${index}");
    let mut project_phs = Vec::with_capacity(projects.len());
    for _ in projects {
        index += 1;
        project_phs.push(format!("${index}"));
    }
    index += 1;
    let member_ph = format!("${index}");
    let extra = match narrow {
        Some(_) => {
            index += 1;
            Some(export_project_narrow_clause(&format!("${index}")))
        }
        None => None,
    };
    let sql = export_issues_sql(&workspace_ph, &project_phs, &member_ph, extra.as_deref());
    let mut query = sqlx::query(&sql).bind(workspace);
    for project in projects {
        query = query.bind(project);
    }
    query = query.bind(member);
    if let Some(narrow) = narrow {
        query = query.bind(narrow);
    }
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|error| format!("issue_export_task: issue query failed ({error})"))?;
    let mut issues = Vec::with_capacity(rows.len());
    for row in rows {
        use sqlx::Row;
        issues.push(IssueRow {
            id: row
                .try_get("id")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            parent_id: row
                .try_get("parent_id")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            project_id: row
                .try_get("project_id")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            project_name: row
                .try_get("project_name")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            project_identifier: row
                .try_get("project_identifier")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            sequence_id: row
                .try_get("sequence_id")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            name: row
                .try_get("name")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            state_name: row
                .try_get("state_name")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            priority: row
                .try_get("priority")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            complexity_score: row
                .try_get("complexity_score")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            estimate_value: row
                .try_get("estimate_value")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            created_first: row
                .try_get("created_first")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            created_last: row
                .try_get("created_last")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            has_creator: row
                .try_get::<Option<Uuid>, _>("creator_id")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?
                .is_some(),
            start_date: row
                .try_get("start_date")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            target_date: row
                .try_get("target_date")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            completed_at: row
                .try_get("completed_at")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            created_at: row
                .try_get("created_at")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            updated_at: row
                .try_get("updated_at")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            archived_at: row
                .try_get("archived_at")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
            is_draft: row
                .try_get("is_draft")
                .map_err(|error| format!("issue_export_task: bad issue row ({error})"))?,
        });
    }
    Ok(issues)
}

/// One issue's prefetched relations, keyed by issue id. Manager
/// semantics per leg (`serializers/issue.py:52-93` over the task
/// queryset's prefetches, `export_task.py:156-189`): assignees ride the
/// M2M through `issue_assignees` — Django does NOT scope
/// explicit-through M2M joins, so soft-deleted links stay, and the
/// serializer keeps active users only (ordered by user `-created_at`);
/// labels/cycles/modules/subscribers/links/comments/relations ride
/// reverse related managers (`SoftDeletionManager`, `deleted_at IS
/// NULL`), with `-created_at` order except comments (`created_at` ASC,
/// the explicit `Prefetch` ordering) and relations (no ordering — plan
/// order, like Django); targets (user/label/cycle/module rows, parents,
/// related issues) resolve by PK with no scope (forward-FK traversal).
/// One raw comment row: stripped text, actor names (both `None` when
/// there is no actor), and the raw timestamp (the shape kernel formats
/// the `strftime` itself).
#[derive(Debug, Clone)]
pub struct CommentRow {
    pub stripped: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub has_actor: bool,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Default)]
pub struct IssueRelations {
    pub assignees: HashMap<Uuid, Vec<(String, String)>>,
    pub labels: HashMap<Uuid, Vec<String>>,
    pub cycles: HashMap<Uuid, Vec<String>>,
    pub modules: HashMap<Uuid, Vec<String>>,
    pub comments: HashMap<Uuid, Vec<CommentRow>>,
    pub subscribers: HashMap<Uuid, Vec<(String, String)>>,
    pub links: HashMap<Uuid, Vec<(String, Option<String>)>>,
    pub outgoing: HashMap<Uuid, Vec<(String, Uuid)>>,
    pub incoming: HashMap<Uuid, Vec<(String, Uuid)>>,
}

async fn fetch_relations(pool: &PgPool, issue_ids: &[Uuid]) -> Result<IssueRelations, String> {
    let mut relations = IssueRelations::default();
    if issue_ids.is_empty() {
        return Ok(relations);
    }
    let failed =
        |leg: &str, error: sqlx::Error| format!("issue_export_task: {leg} query failed ({error})");
    // Assignees: unscoped through join, active users, user -created_at.
    let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT \"issue_assignees\".\"issue_id\", \"users\".\"first_name\", \
         \"users\".\"last_name\" FROM \"issue_assignees\" \
         INNER JOIN \"users\" ON (\"issue_assignees\".\"assignee_id\" = \"users\".\"id\") \
         WHERE \"issue_assignees\".\"issue_id\" = ANY($1) AND \"users\".\"is_active\" \
         ORDER BY \"users\".\"created_at\" DESC",
    )
    .bind(issue_ids)
    .fetch_all(pool)
    .await
    .map_err(|error| failed("assignee", error))?;
    for (issue_id, first, last) in rows {
        relations
            .assignees
            .entry(issue_id)
            .or_default()
            .push((first, last));
    }
    // Labels / cycles / modules: scoped through rows, through -created_at.
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT \"issue_labels\".\"issue_id\", \"labels\".\"name\" FROM \"issue_labels\" \
         INNER JOIN \"labels\" ON (\"issue_labels\".\"label_id\" = \"labels\".\"id\") \
         WHERE \"issue_labels\".\"issue_id\" = ANY($1) \
         AND \"issue_labels\".\"deleted_at\" IS NULL \
         ORDER BY \"issue_labels\".\"created_at\" DESC",
    )
    .bind(issue_ids)
    .fetch_all(pool)
    .await
    .map_err(|error| failed("label", error))?;
    for (issue_id, name) in rows {
        relations.labels.entry(issue_id).or_default().push(name);
    }
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT \"cycle_issues\".\"issue_id\", \"cycles\".\"name\" FROM \"cycle_issues\" \
         INNER JOIN \"cycles\" ON (\"cycle_issues\".\"cycle_id\" = \"cycles\".\"id\") \
         WHERE \"cycle_issues\".\"issue_id\" = ANY($1) \
         AND \"cycle_issues\".\"deleted_at\" IS NULL \
         ORDER BY \"cycle_issues\".\"created_at\" DESC",
    )
    .bind(issue_ids)
    .fetch_all(pool)
    .await
    .map_err(|error| failed("cycle", error))?;
    for (issue_id, name) in rows {
        relations.cycles.entry(issue_id).or_default().push(name);
    }
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT \"module_issues\".\"issue_id\", \"modules\".\"name\" FROM \"module_issues\" \
         INNER JOIN \"modules\" ON (\"module_issues\".\"module_id\" = \"modules\".\"id\") \
         WHERE \"module_issues\".\"issue_id\" = ANY($1) \
         AND \"module_issues\".\"deleted_at\" IS NULL \
         ORDER BY \"module_issues\".\"created_at\" DESC",
    )
    .bind(issue_ids)
    .fetch_all(pool)
    .await
    .map_err(|error| failed("module", error))?;
    for (issue_id, name) in rows {
        relations.modules.entry(issue_id).or_default().push(name);
    }
    // Comments: scoped, actor left-joined, created_at ASC.
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        Uuid,
        String,
        Option<Uuid>,
        Option<String>,
        Option<String>,
        Option<DateTime<Utc>>,
    )> = sqlx::query_as(
        "SELECT \"issue_comments\".\"issue_id\", \
         \"issue_comments\".\"comment_stripped\", \"issue_comments\".\"actor_id\", \
         \"users\".\"first_name\", \"users\".\"last_name\", \
         \"issue_comments\".\"created_at\" \
         FROM \"issue_comments\" \
         LEFT OUTER JOIN \"users\" ON (\"issue_comments\".\"actor_id\" = \"users\".\"id\") \
         WHERE \"issue_comments\".\"issue_id\" = ANY($1) \
         AND \"issue_comments\".\"deleted_at\" IS NULL \
         ORDER BY \"issue_comments\".\"created_at\" ASC",
    )
    .bind(issue_ids)
    .fetch_all(pool)
    .await
    .map_err(|error| failed("comment", error))?;
    for (issue_id, stripped, actor_id, first_name, last_name, created_at) in rows {
        relations
            .comments
            .entry(issue_id)
            .or_default()
            .push(CommentRow {
                stripped,
                first_name,
                last_name,
                has_actor: actor_id.is_some(),
                created_at,
            });
    }
    // Subscribers: scoped, through -created_at.
    let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT \"issue_subscribers\".\"issue_id\", \"users\".\"first_name\", \
         \"users\".\"last_name\" FROM \"issue_subscribers\" \
         INNER JOIN \"users\" ON (\"issue_subscribers\".\"subscriber_id\" = \"users\".\"id\") \
         WHERE \"issue_subscribers\".\"issue_id\" = ANY($1) \
         AND \"issue_subscribers\".\"deleted_at\" IS NULL \
         ORDER BY \"issue_subscribers\".\"created_at\" DESC",
    )
    .bind(issue_ids)
    .fetch_all(pool)
    .await
    .map_err(|error| failed("subscriber", error))?;
    for (issue_id, first, last) in rows {
        relations
            .subscribers
            .entry(issue_id)
            .or_default()
            .push((first, last));
    }
    // Links: scoped, through -created_at.
    let rows: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT \"issue_id\", \"url\", \"title\" FROM \"issue_links\" \
         WHERE \"issue_id\" = ANY($1) AND \"deleted_at\" IS NULL \
         ORDER BY \"created_at\" DESC",
    )
    .bind(issue_ids)
    .fetch_all(pool)
    .await
    .map_err(|error| failed("link", error))?;
    for (issue_id, url, title) in rows {
        relations
            .links
            .entry(issue_id)
            .or_default()
            .push((url, title));
    }
    // Relations, both directions: scoped, unordered like Django.
    let rows: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT \"issue_id\", \"related_issue_id\", \"relation_type\" \
         FROM \"issue_relations\" \
         WHERE \"issue_id\" = ANY($1) AND \"deleted_at\" IS NULL",
    )
    .bind(issue_ids)
    .fetch_all(pool)
    .await
    .map_err(|error| failed("relation", error))?;
    for (issue_id, related_id, relation) in rows {
        relations
            .outgoing
            .entry(issue_id)
            .or_default()
            .push((relation, related_id));
    }
    let rows: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT \"related_issue_id\", \"issue_id\", \"relation_type\" \
         FROM \"issue_relations\" \
         WHERE \"related_issue_id\" = ANY($1) AND \"deleted_at\" IS NULL",
    )
    .bind(issue_ids)
    .fetch_all(pool)
    .await
    .map_err(|error| failed("relation", error))?;
    for (issue_id, other_id, relation) in rows {
        relations
            .incoming
            .entry(issue_id)
            .or_default()
            .push((relation, other_id));
    }
    Ok(relations)
}

/// Resolve display identifiers (`IDENT-SEQ`) for parents and related
/// issues by PK with no scope (forward-FK traversal sees deleted rows).
async fn fetch_identifiers(
    pool: &PgPool,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, (String, i64)>, String> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Uuid, String, i32)> = sqlx::query_as(
        "SELECT \"issues\".\"id\", \"projects\".\"identifier\", \"issues\".\"sequence_id\" \
         FROM \"issues\" \
         INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") \
         WHERE \"issues\".\"id\" = ANY($1)",
    )
    .bind(ids)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("issue_export_task: identifier query failed ({error})"))?;
    Ok(rows
        .into_iter()
        .map(|(id, identifier, sequence)| (id, (identifier, i64::from(sequence))))
        .collect())
}

/// Map `(relation_type, other issue id)` pairs to kernel relation
/// inputs, resolving display identifiers (`IDENT-SEQ`); unresolvable
/// ends map to `None` and are skipped by the kernel.
fn rel_inputs<'x>(
    pairs: &'x [(String, Uuid)],
    identifiers: &'x HashMap<Uuid, (String, i64)>,
) -> Vec<export_shape::RelationInput<'x>> {
    pairs
        .iter()
        .map(|(relation, other)| export_shape::RelationInput {
            relation_type: Some(relation.as_str()),
            issue: identifiers
                .get(other)
                .map(|(identifier, sequence)| (identifier.as_str(), *sequence)),
        })
        .collect()
}

/// Assemble one serializer row from the issue columns plus its
/// relations, in [`EXPORT_ROW_FIELDS`] order with porter values. The
/// three `Count` keys are omitted (BUG-EXPORT-1, see
/// [`EXPORT_ROW_FIELDS`]).
pub fn assemble_row(
    issue: &IssueRow,
    relations: &IssueRelations,
    identifiers: &HashMap<Uuid, (String, i64)>,
) -> Vec<(String, porter::PVal)> {
    use export_shape::{CommentInput, EstimatePoint, IssueExportRow};
    // Owned temporaries the borrowed kernel inputs point at.
    let assignee_names: Vec<String> = relations
        .assignees
        .get(&issue.id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|(first, last)| full_name(Some(&first), Some(&last)))
        .collect();
    let assignees: Vec<(&str, bool)> = assignee_names
        .iter()
        .map(|name| (name.as_str(), true))
        .collect();
    let subscriber_names: Vec<String> = relations
        .subscribers
        .get(&issue.id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|(first, last)| full_name(Some(&first), Some(&last)))
        .collect();
    let subscribers: Vec<Option<&str>> = subscriber_names
        .iter()
        .map(|name| Some(name.as_str()))
        .collect();
    let creator = if issue.has_creator {
        full_name(
            issue.created_first.as_deref(),
            issue.created_last.as_deref(),
        )
    } else {
        String::new()
    };
    let start = drf_date(issue.start_date);
    let target = drf_date(issue.target_date);
    let completed = drf_datetime(issue.completed_at);
    let created = drf_datetime(Some(issue.created_at)).unwrap_or_default();
    let updated = drf_datetime(Some(issue.updated_at)).unwrap_or_default();
    let archived = drf_date(issue.archived_at);
    // `estimate_point.value` renders verbatim (a `CharField` string);
    // a null point renders `""` (the kernel's `None` arm).
    let estimate_json = issue
        .estimate_value
        .as_deref()
        .map(|text| serde_json::Value::String(text.to_owned()));
    let labels: Vec<(&str, Option<&str>)> = relations
        .labels
        .get(&issue.id)
        .map(|names| names.iter().map(|name| (name.as_str(), None)).collect())
        .unwrap_or_default();
    let cycles: Vec<&str> = relations
        .cycles
        .get(&issue.id)
        .map(|names| names.iter().map(String::as_str).collect())
        .unwrap_or_default();
    let modules: Vec<&str> = relations
        .modules
        .get(&issue.id)
        .map(|names| names.iter().map(String::as_str).collect())
        .unwrap_or_default();
    let links: Vec<(&str, Option<&str>)> = relations
        .links
        .get(&issue.id)
        .map(|pairs| {
            pairs
                .iter()
                .map(|(url, title)| (url.as_str(), title.as_deref()))
                .collect()
        })
        .unwrap_or_default();
    let empty_rels: Vec<(String, Uuid)> = Vec::new();
    let outgoing = rel_inputs(
        relations.outgoing.get(&issue.id).unwrap_or(&empty_rels),
        identifiers,
    );
    let incoming = rel_inputs(
        relations.incoming.get(&issue.id).unwrap_or(&empty_rels),
        identifiers,
    );
    let author_names: Vec<String> = relations
        .comments
        .get(&issue.id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|comment| {
            if comment.has_actor {
                full_name(comment.first_name.as_deref(), comment.last_name.as_deref())
            } else {
                String::new()
            }
        })
        .collect();
    let empty_comments: Vec<CommentRow> = Vec::new();
    let comment_rows = relations.comments.get(&issue.id).unwrap_or(&empty_comments);
    let comments: Vec<CommentInput<'_>> = comment_rows
        .iter()
        .zip(author_names.iter())
        .map(|(comment, author)| CommentInput {
            stripped: Some(comment.stripped.as_str()),
            html: "",
            author_full_name: if comment.has_actor {
                Some(author.as_str())
            } else {
                None
            },
            created_at: comment.created_at,
        })
        .collect();
    let parent = issue.parent_id.as_ref().and_then(|id| {
        identifiers
            .get(id)
            .map(|(identifier, sequence)| (identifier.as_str(), *sequence))
    });
    let row = IssueExportRow {
        project_name: &issue.project_name,
        project_identifier: &issue.project_identifier,
        parent,
        sequence_id: i64::from(issue.sequence_id),
        name: &issue.name,
        state_name: issue.state_name.as_deref().unwrap_or(""),
        priority: &issue.priority,
        complexity_score: Some(i64::from(issue.complexity_score)),
        assignees: &assignees,
        subscribers: &subscribers,
        created_by_name: &creator,
        start_date: start.as_deref(),
        target_date: target.as_deref(),
        completed_at: completed.as_deref(),
        created_at: &created,
        updated_at: &updated,
        archived_at: archived.as_deref(),
        estimate: estimate_json.as_ref().map(EstimatePoint::WithValue),
        labels: &labels,
        cycles: &cycles,
        modules: &modules,
        links: &links,
        outgoing_relations: &outgoing,
        incoming_relations: &incoming,
        comments: &comments,
        // Unreachable inputs: the counts never render (BUG-EXPORT-1).
        sub_issues_count: 0,
        link_count: 0,
        attachment_count: 0,
        is_draft: issue.is_draft,
    };
    let view = export_shape::issue_export_to_representation(&row);
    view_to_pairs(&view)
}

/// Map one serialized view to porter rows in [`EXPORT_ROW_FIELDS`]
/// order. Nullable dates/datetimes render `Null` when absent (DRF's
/// `None` output); the three `Count` keys never appear (BUG-EXPORT-1).
fn view_to_pairs(view: &export_shape::IssueExportView<'_>) -> Vec<(String, porter::PVal)> {
    use porter::PVal;
    let text = |value: &str| PVal::Str(value.to_owned());
    let opt = |value: Option<&str>| value.map(text).unwrap_or(PVal::Null);
    let list = |items: &[&str]| PVal::List(items.iter().map(|item| text(item)).collect());
    vec![
        ("project_name".to_owned(), text(view.project_name)),
        (
            "project_identifier".to_owned(),
            text(view.project_identifier),
        ),
        ("parent".to_owned(), text(&view.parent)),
        ("identifier".to_owned(), text(&view.identifier)),
        ("sequence_id".to_owned(), PVal::Int(view.sequence_id)),
        ("name".to_owned(), text(view.name)),
        ("state_name".to_owned(), text(view.state_name)),
        ("priority".to_owned(), text(view.priority)),
        (
            "complexity_score".to_owned(),
            view.complexity_score.map(PVal::Int).unwrap_or(PVal::Null),
        ),
        ("assignees".to_owned(), list(&view.assignees)),
        ("subscribers".to_owned(), list(&view.subscribers)),
        ("created_by_name".to_owned(), text(view.created_by_name)),
        ("start_date".to_owned(), opt(view.start_date)),
        ("target_date".to_owned(), opt(view.target_date)),
        ("completed_at".to_owned(), opt(view.completed_at)),
        ("created_at".to_owned(), text(view.created_at)),
        ("updated_at".to_owned(), text(view.updated_at)),
        ("archived_at".to_owned(), opt(view.archived_at)),
        ("estimate".to_owned(), PVal::from_json(&view.estimate)),
        ("labels".to_owned(), list(&view.labels)),
        ("cycles".to_owned(), list(&view.cycles)),
        ("modules".to_owned(), list(&view.modules)),
        (
            "links".to_owned(),
            PVal::List(
                view.links
                    .iter()
                    .map(|link| {
                        PVal::Dict(vec![
                            ("url".to_owned(), text(link.url)),
                            ("title".to_owned(), text(link.title)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "relations".to_owned(),
            PVal::List(
                view.relations
                    .iter()
                    .map(|relation| {
                        PVal::Dict(vec![
                            ("type".to_owned(), text(&relation.relation_type)),
                            ("issue".to_owned(), text(&relation.issue)),
                            ("direction".to_owned(), text(relation.direction)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "comments".to_owned(),
            PVal::List(
                view.comments
                    .iter()
                    .map(|comment| {
                        PVal::Dict(vec![
                            ("comment".to_owned(), text(comment.comment)),
                            ("created_by".to_owned(), text(comment.created_by)),
                            ("created_at".to_owned(), text(&comment.created_at)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("is_draft".to_owned(), PVal::Bool(view.is_draft)),
    ]
}

/// Encode one project file (`export_task.py:204-215` + `DataExporter`):
/// serialize → porter engine → `(inner filename, bytes)`.
pub fn encode_project_file(
    rows: &[Vec<(String, porter::PVal)>],
    provider: &str,
    inner_name: &str,
) -> Result<(String, Vec<u8>), String> {
    let (filename, content) = porter::porter_export(provider, inner_name, rows)?;
    let bytes = match content {
        porter::PorterContent::Text(text) => text.into_bytes(),
        porter::PorterContent::Bytes(bytes) => bytes,
    };
    Ok((filename, bytes))
}

/// Execute one signed PUT (`upload_fileobj` single-part shape for zips
/// under the 8 MiB multipart threshold — identical object bytes either
/// way): `PUT url` with the signed headers + content SHA. Any transport
/// or status failure is the task error (boto raises the same way).
pub async fn put_object(
    signed: &pidash_storage::SignedRequest,
    payload: &[u8],
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let mut request = client
        .put(&signed.url)
        .header("authorization", &signed.authorization)
        .header("x-amz-date", &signed.amz_date)
        .header("x-amz-content-sha256", &signed.content_sha256);
    if let Some(content_type) = &signed.content_type {
        request = request.header("content-type", content_type);
    }
    for (name, value) in &signed.extra_headers {
        request = request.header(name.as_str(), value.as_str());
    }
    let response = request
        .body(payload.to_vec())
        .send()
        .await
        .map_err(|error| format!("issue_export_task: S3 upload failed ({error})"))?;
    if !response.status().is_success() {
        return Err(format!(
            "issue_export_task: S3 upload failed (HTTP {})",
            response.status()
        ));
    }
    Ok(())
}

/// `upload_to_s3` (`export_task.py:20-125`): PUT the zip, then mint the
/// presigned GET URL for the resolved branch. Returns `(url, key)`.
/// The PUT and the presign run in Python's order (the MinIO presign
/// endpoint resolves AFTER the upload, so its `AttributeError` still
/// surfaces post-upload).
pub async fn upload_to_s3(
    s3: &ExportS3,
    key: &str,
    payload: &[u8],
    now: DateTime<Utc>,
) -> Result<(String, String), String> {
    use pidash_storage::{presign_get_url, sign_put, sign_put_extra, PresignEndpoint};
    // The PUT settings: `use_minio: false` always, so the resolver takes
    // the endpoint-URL arm (path-style `{endpoint}/{bucket}/{key}` —
    // the probed boto3 shape for both the MinIO and endpoint uploads).
    // The MinIO-ness of the upload is the `x-amz-acl` header alone.
    let put_storage = pidash_db::config::StorageSettings {
        use_minio: false,
        minio_endpoint_ssl: false,
        access_key_id: s3.access_key_id.clone(),
        secret_access_key: s3.secret_access_key.clone(),
        bucket_name: s3.bucket.clone(),
        region: s3.client_region(),
        endpoint_url: s3.endpoint_url.clone(),
        signed_url_expiration_secs: 604800,
    };
    let amz_datetime = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    match upload_branch(s3) {
        UploadBranch::Minio => {
            let signed = sign_put_extra(
                &put_storage,
                "http",
                "localhost",
                key,
                "application/zip",
                payload,
                &[("x-amz-acl", "public-read")],
                &amz_datetime,
                &date_stamp,
            )
            .map_err(|error| format!("issue_export_task: S3 sign failed ({error})"))?;
            put_object(&signed, payload).await?;
            let (protocol, custom_domain) = minio_presign_parts(s3)?;
            let endpoint = ex::minio_presign_endpoint(&protocol, &custom_domain);
            let (base, host) = endpoint_parts(&endpoint)?;
            let url = presign_get_url(
                &PresignEndpoint {
                    url_base: base,
                    signed_host: host,
                    path_style: true,
                },
                &s3.bucket,
                key,
                &s3.access_key_id,
                &s3.secret_access_key,
                &s3.client_region(),
                &amz_datetime,
                &date_stamp,
                dto::EXPORT_URL_EXPIRES_IN_SECS,
            );
            Ok((url, key.to_owned()))
        }
        UploadBranch::Endpoint => {
            let signed = sign_put(
                &put_storage,
                "http",
                "localhost",
                key,
                "application/zip",
                payload,
                &amz_datetime,
                &date_stamp,
            )
            .map_err(|error| format!("issue_export_task: S3 sign failed ({error})"))?;
            put_object(&signed, payload).await?;
            let endpoint_url = s3.endpoint_url.clone().unwrap_or_default();
            let (base, host) = endpoint_parts(&endpoint_url)?;
            let url = presign_get_url(
                &PresignEndpoint {
                    url_base: base,
                    signed_host: host,
                    path_style: true,
                },
                &s3.bucket,
                key,
                &s3.access_key_id,
                &s3.secret_access_key,
                &s3.client_region(),
                &amz_datetime,
                &date_stamp,
                dto::EXPORT_URL_EXPIRES_IN_SECS,
            );
            Ok((url, key.to_owned()))
        }
        UploadBranch::Region => {
            if s3.region_setting.is_empty() {
                // `region_name=""` → botocore raises `ValueError` at
                // client creation (probed verbatim).
                return Err("Invalid endpoint: https://s3..amazonaws.com".to_owned());
            }
            let storage = pidash_db::config::StorageSettings {
                region: s3.region_setting.clone(),
                endpoint_url: None,
                ..put_storage.clone()
            };
            let signed = sign_put(
                &storage,
                "https",
                "localhost",
                key,
                "application/zip",
                payload,
                &amz_datetime,
                &date_stamp,
            )
            .map_err(|error| format!("issue_export_task: S3 sign failed ({error})"))?;
            put_object(&signed, payload).await?;
            let (base, host) = region_base(&s3.bucket, &s3.region_setting);
            let url = presign_get_url(
                &PresignEndpoint {
                    url_base: base,
                    signed_host: host,
                    path_style: false,
                },
                &s3.bucket,
                key,
                &s3.access_key_id,
                &s3.secret_access_key,
                &s3.region_setting,
                &amz_datetime,
                &date_stamp,
                dto::EXPORT_URL_EXPIRES_IN_SECS,
            );
            Ok((url, key.to_owned()))
        }
    }
}

/// One exporter row: `ExporterHistory.objects.get(token=...)`
/// (`export_task.py:143`).
pub struct ExporterRow {
    pub id: Uuid,
    pub initiated_by: Uuid,
}

async fn fetch_exporter(pool: &PgPool, token: &str) -> Result<ExporterRow, String> {
    let sql = fetch_exporter_by_token_sql("$1");
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(&sql)
        .bind(token)
        .fetch_all(pool)
        .await
        .map_err(|error| format!("issue_export_task: exporter query failed ({error})"))?;
    if rows.is_empty() {
        // `.get()` misses: the outer `except`'s own `.get()` re-raises,
        // so the task dies uncaught (Celery FAILURE, no redelivery).
        return Err("issue_export_task: ExporterHistory matching query does not exist.".to_owned());
    }
    if rows.len() > 1 {
        return Err("issue_export_task: get() returned more than one ExporterHistory.".to_owned());
    }
    let (id, initiated_by) = rows[0];
    Ok(ExporterRow { id, initiated_by })
}

async fn mark_processing(pool: &PgPool, id: Uuid) -> Result<(), String> {
    sqlx::query(&mark_processing_sql("$1", "$2"))
        .bind(STATUS_PROCESSING)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|error| format!("issue_export_task: status write failed ({error})"))?;
    Ok(())
}

async fn mark_failed(pool: &PgPool, id: Uuid, reason: &str) -> Result<(), String> {
    sqlx::query(&mark_failed_sql("$1", "$2", "$3"))
        .bind(STATUS_FAILED)
        .bind(reason)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|error| format!("issue_export_task: status write failed ({error})"))?;
    Ok(())
}

async fn mark_uploaded(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    url: &str,
    key: &str,
) -> Result<(), String> {
    sqlx::query(&mark_uploaded_sql("$1", "$2", "$3", "$4"))
        .bind(status)
        .bind(url)
        .bind(key)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|error| format!("issue_export_task: status write failed ({error})"))?;
    Ok(())
}

/// Serialize one issue set to porter rows (`IssueExportSerializer` +
/// `DataExporter.export` row collection, `export_task.py:190-215`).
async fn serialize_issues(
    pool: &PgPool,
    workspace: Uuid,
    projects: &[Uuid],
    member: Uuid,
    narrow: Option<Uuid>,
) -> Result<Vec<Vec<(String, porter::PVal)>>, String> {
    let issues = fetch_issues(pool, workspace, projects, member, narrow).await?;
    let issue_ids: Vec<Uuid> = issues.iter().map(|issue| issue.id).collect();
    let relations = fetch_relations(pool, &issue_ids).await?;
    let mut lookups: Vec<Uuid> = issues.iter().filter_map(|issue| issue.parent_id).collect();
    for issue in &issues {
        for (_, other) in relations
            .outgoing
            .get(&issue.id)
            .into_iter()
            .flatten()
            .chain(relations.incoming.get(&issue.id).into_iter().flatten())
        {
            lookups.push(*other);
        }
    }
    lookups.sort();
    lookups.dedup();
    let identifiers = fetch_identifiers(pool, &lookups).await?;
    Ok(issues
        .iter()
        .map(|issue| assemble_row(issue, &relations, &identifiers))
        .collect())
}

/// Run one bound export. `Ok(Outcome::Done)` settles to [`Verdict::Ack`];
/// `Ok(Outcome::Uncaught)` (the missing-row path Python cannot handle)
/// settles to [`Verdict::Fail`]. Every other error marks the row
/// `failed` + reason first (the mark itself failing also settles Fail).
enum Outcome {
    Done,
    Uncaught(String),
}

async fn run_issue_export(
    pool: &PgPool,
    call: &ExportTaskCall,
    s3: &ExportS3,
    now: DateTime<Utc>,
) -> Result<Outcome, String> {
    let exporter = match fetch_exporter(pool, &call.token_id).await {
        Ok(exporter) => exporter,
        Err(error) => return Ok(Outcome::Uncaught(error)),
    };
    if let Err(error) = mark_processing(pool, exporter.id).await {
        return Ok(Outcome::Uncaught(error));
    }
    // UUID coercion in filter order (workspace, projects, member).
    let workspace = match wire_uuid(&call.workspace_id) {
        Ok(id) => id,
        Err(reason) => {
            mark_failed(pool, exporter.id, &reason).await?;
            return Ok(Outcome::Done);
        }
    };
    let mut projects = Vec::with_capacity(call.project_ids.len());
    for raw in &call.project_ids {
        match wire_uuid(raw) {
            Ok(id) => projects.push(id),
            Err(reason) => {
                mark_failed(pool, exporter.id, &reason).await?;
                return Ok(Outcome::Done);
            }
        }
    }
    if let Err(reason) = validate_task_provider(&call.provider) {
        mark_failed(pool, exporter.id, &reason).await?;
        return Ok(Outcome::Done);
    }
    // Serialize (single or per-project), zip, upload.
    let built = build_export_archive(pool, call, &exporter, &workspace, &projects).await;
    let files = match built {
        Ok(files) => files,
        Err(reason) => {
            mark_failed(pool, exporter.id, &reason).await?;
            return Ok(Outcome::Done);
        }
    };
    let entries: Vec<ex::ZipEntry<'_>> = files
        .iter()
        .map(|(name, content)| ex::ZipEntry {
            name: name.as_str(),
            content: content.as_slice(),
        })
        .collect();
    let zip = ex::build_zip(&entries);
    let today = now.date_naive().to_string();
    let key = super::export_s3_key(&call.workspace_id, &call.slug, &call.token_id, &today);
    match upload_to_s3(s3, &key, &zip, now).await {
        Ok((url, key)) => {
            let update = ex::resolve_upload_update(Some(url.as_str()), &key);
            // `save(update_fields=["status", "url", "key"])`: the
            // completed branch always carries `Some` (the URL is
            // truthy); the failed branch writes `None`s — but the
            // columns are non-null (`TextField(blank=True)`), so Django
            // would crash there. In practice `upload_to_s3` returns a
            // URL or raises, making the `None` arm unreachable; the
            // unwrap documents that (a panic would surface as a task
            // error, like the Django crash).
            let url = update.url.unwrap_or_default();
            let key = update.key.unwrap_or_default();
            mark_uploaded(pool, exporter.id, update.status, &url, &key).await?;
            Ok(Outcome::Done)
        }
        Err(reason) => {
            mark_failed(pool, exporter.id, &reason).await?;
            Ok(Outcome::Done)
        }
    }
}

/// Build the `(filename, bytes)` files for the zip: one per project id
/// when `multiple`, else a single workspace file
/// (`export_task.py:202-216`).
async fn build_export_archive(
    pool: &PgPool,
    call: &ExportTaskCall,
    exporter: &ExporterRow,
    workspace: &Uuid,
    projects: &[Uuid],
) -> Result<Vec<(String, Vec<u8>)>, String> {
    if call.multiple {
        let mut files = Vec::with_capacity(call.project_ids.len());
        for (raw, project) in call.project_ids.iter().zip(projects.iter()) {
            let rows = serialize_issues(
                pool,
                *workspace,
                projects,
                exporter.initiated_by,
                Some(*project),
            )
            .await?;
            let inner = task_export_filename(&call.slug, true, raw, &call.workspace_id);
            files.push(encode_project_file(&rows, &call.provider, &inner)?);
        }
        Ok(files)
    } else {
        let rows =
            serialize_issues(pool, *workspace, projects, exporter.initiated_by, None).await?;
        let inner = task_export_filename(&call.slug, false, "", &call.workspace_id);
        Ok(vec![encode_project_file(&rows, &call.provider, &inner)?])
    }
}

/// Handler for [`ISSUE_EXPORT_TASK`]:
/// `issue_export_task(provider, workspace_id, project_ids, token_id,
/// multiple, slug)`.
pub fn issue_export_handler(pool: PgPool) -> Handler {
    Arc::new(move |job: JobRow| {
        let pool = pool.clone();
        Box::pin(async move {
            let call = match bind_export_task_call(&job.args, &job.kwargs) {
                Ok(call) => call,
                Err(error) => return Err(error),
            };
            let outcome = run_issue_export(&pool, &call, &ExportS3::from_env(), Utc::now()).await;
            match outcome {
                Ok(Outcome::Done) => Ok(Verdict::Ack),
                Ok(Outcome::Uncaught(error)) => Ok(Verdict::Fail { error }),
                Err(error) => Ok(Verdict::Fail { error }),
            }
        })
    })
}

/// Register [`ISSUE_EXPORT_TASK`] on `registry` (bare `@shared_task`).
pub fn register_issue_export_task(registry: &mut crate::worker::Registry, pool: PgPool) {
    registry.register(ISSUE_EXPORT_TASK, issue_export_handler(pool));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue_row() -> IssueRow {
        IssueRow {
            id: Uuid::nil(),
            parent_id: None,
            project_id: Uuid::nil(),
            project_name: "Site".to_owned(),
            project_identifier: "ST".to_owned(),
            sequence_id: 5,
            name: "Login bug".to_owned(),
            state_name: Some("Todo".to_owned()),
            priority: "high".to_owned(),
            complexity_score: 3,
            estimate_value: Some("5".to_owned()),
            created_first: Some("Ada".to_owned()),
            created_last: Some("Lovelace".to_owned()),
            has_creator: true,
            start_date: Some(NaiveDate::from_ymd_opt(2026, 1, 2).unwrap()),
            target_date: None,
            completed_at: None,
            created_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            updated_at: DateTime::from_timestamp(1_700_000_100, 500_000_000).unwrap(),
            archived_at: None,
            is_draft: false,
        }
    }

    #[test]
    fn row_fields_are_meta_order_minus_counts() {
        // BUG-EXPORT-1: the three unannotated counts never render.
        assert_eq!(EXPORT_ROW_FIELDS.len(), 26);
        assert!(!EXPORT_ROW_FIELDS.contains(&"sub_issues_count"));
        assert!(!EXPORT_ROW_FIELDS.contains(&"link_count"));
        assert!(!EXPORT_ROW_FIELDS.contains(&"attachment_count"));
        assert_eq!(
            &EXPORT_ROW_FIELDS[..6],
            &[
                "project_name",
                "project_identifier",
                "parent",
                "identifier",
                "sequence_id",
                "name"
            ]
        );
        assert_eq!(EXPORT_ROW_FIELDS[25], "is_draft");
    }

    #[test]
    fn drf_renders_match_probed_drf() {
        assert_eq!(
            drf_datetime(Some(DateTime::from_timestamp(1_704_079_845, 0).unwrap())),
            Some("2024-01-01T03:30:45Z".to_owned())
        );
        assert_eq!(
            drf_datetime(Some(
                DateTime::from_timestamp(1_704_079_845, 123_456_000).unwrap()
            )),
            Some("2024-01-01T03:30:45.123456Z".to_owned())
        );
        assert_eq!(drf_datetime(None), None);
        assert_eq!(
            drf_date(Some(NaiveDate::from_ymd_opt(2026, 1, 2).unwrap())),
            Some("2026-01-02".to_owned())
        );
        assert_eq!(drf_date(None), None);
        assert_eq!(full_name(Some("Ada"), Some("Lovelace")), "Ada Lovelace");
        assert_eq!(full_name(Some("Ada"), Some("")), "Ada");
        assert_eq!(full_name(None, None), "None None");
    }

    #[test]
    fn wire_uuid_rejects_like_django() {
        let id = Uuid::nil();
        assert_eq!(wire_uuid(&id.to_string()).unwrap(), id);
        assert_eq!(
            wire_uuid("bogus").unwrap_err(),
            "[\"'bogus' is not a valid UUID.\"]"
        );
    }

    #[test]
    fn branches_resolve_in_python_order() {
        let base = ExportS3 {
            use_minio: false,
            endpoint_url: None,
            access_key_id: String::new(),
            secret_access_key: String::new(),
            bucket: "uploads".to_owned(),
            region_setting: String::new(),
            web_url: "http://localhost".to_owned(),
            default_region: None,
        };
        assert_eq!(upload_branch(&base), UploadBranch::Region);
        let endpoint = ExportS3 {
            endpoint_url: Some("http://minio:9000".to_owned()),
            ..base.clone()
        };
        assert_eq!(upload_branch(&endpoint), UploadBranch::Endpoint);
        let minio = ExportS3 {
            use_minio: true,
            ..endpoint
        };
        assert_eq!(upload_branch(&minio), UploadBranch::Minio);
        // MinIO wins over the endpoint URL (first branch).
        assert_eq!(base.client_region(), "us-east-1");
        let pinned = ExportS3 {
            default_region: Some("eu-west-1".to_owned()),
            ..base
        };
        assert_eq!(pinned.client_region(), "eu-west-1");
    }

    #[test]
    fn minio_presign_parts_derive_from_web_url() {
        let s3 = ExportS3 {
            use_minio: true,
            endpoint_url: Some("http://minio:9000".to_owned()),
            access_key_id: String::new(),
            secret_access_key: String::new(),
            bucket: "uploads".to_owned(),
            region_setting: String::new(),
            web_url: "http://localhost:8123".to_owned(),
            default_region: None,
        };
        assert_eq!(
            minio_presign_parts(&s3).unwrap(),
            ("http:".to_owned(), "localhost:8123/uploads".to_owned())
        );
        // No endpoint URL: the attribute lookup raises (ported error).
        let bare = ExportS3 {
            endpoint_url: None,
            ..s3
        };
        assert!(minio_presign_parts(&bare).is_err());
        // Region bases: global endpoint for us-east-1, regional else.
        assert_eq!(
            region_base("uploads", "us-east-1"),
            (
                "https://uploads.s3.amazonaws.com".to_owned(),
                "uploads.s3.amazonaws.com".to_owned()
            )
        );
        assert_eq!(
            region_base("uploads", "us-west-2"),
            (
                "https://uploads.s3.us-west-2.amazonaws.com".to_owned(),
                "uploads.s3.us-west-2.amazonaws.com".to_owned()
            )
        );
    }

    #[test]
    fn issue_statement_shares_where_and_orders() {
        let sql = export_issues_sql("$1", &["$2".to_owned()], "$3", None);
        assert!(sql.contains(&super::super::export_issues_where_sql(
            "$1",
            &["$2".to_owned()],
            "$3"
        )));
        assert!(sql.contains("ORDER BY \"issues\".\"created_at\" DESC"));
        let narrowed = export_issues_sql(
            "$1",
            &["$2".to_owned()],
            "$3",
            Some("\"issues\".\"project_id\" = $4"),
        );
        assert!(narrowed.contains("\"issues\".\"project_id\" = $4"));
    }

    #[test]
    fn assembled_row_matches_serializer_shape() {
        let issue = issue_row();
        let relations = IssueRelations::default();
        let pairs = assemble_row(&issue, &relations, &HashMap::new());
        let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, EXPORT_ROW_FIELDS);
        let get = |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        use porter::PVal;
        assert_eq!(get("identifier"), PVal::Str("ST-5".to_owned()));
        assert_eq!(get("sequence_id"), PVal::Int(5));
        assert_eq!(get("parent"), PVal::Str(String::new()));
        assert_eq!(get("estimate"), PVal::Str("5".to_owned()));
        assert_eq!(get("created_by_name"), PVal::Str("Ada Lovelace".to_owned()));
        assert_eq!(get("start_date"), PVal::Str("2026-01-02".to_owned()));
        assert_eq!(get("target_date"), PVal::Null);
        assert_eq!(get("is_draft"), PVal::Bool(false));
        assert_eq!(get("labels"), PVal::List(vec![]));
        // Golden JSON: key order + exact bytes for one row.
        let json = porter::porter_json_encode_indent(&[pairs], 2);
        assert!(json.contains("\"identifier\": \"ST-5\""));
        assert!(json.contains("\"sequence_id\": 5"));
        assert!(!json.contains("sub_issues_count"));
    }

    #[test]
    fn encode_names_files_per_provider() {
        let (name, _) = encode_project_file(&[], "csv", "an_ws-1").expect("csv");
        assert_eq!(name, "an_ws-1.csv");
        let (name, _) = encode_project_file(&[], "json", "an_ws-1").expect("json");
        assert_eq!(name, "an_ws-1.json");
        let (name, _) = encode_project_file(&[], "xlsx", "an_ws-1").expect("xlsx");
        assert_eq!(name, "an_ws-1.xlsx");
        assert!(encode_project_file(&[], "xml", "an_ws-1").is_err());
    }
}
