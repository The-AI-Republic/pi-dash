//! Issue-export background task wire (D-35, jobs layer, PIDASHCONV-381).
//!
//! Port of the `@shared_task` entry point of
//! `apps/api/pi_dash/bgtasks/export_task.py:127-226`
//! (`issue_export_task`) plus the pure decisions of its helpers
//! `create_zip_file:28-38` and `upload_to_s3:42-124`. Fixtures FX-A-T-01
//! and FX-A-T-02
//! (`rust-api/fixtures/app_analytics/tasks/export_tasks.golden.json`).
//!
//! This module owns the Celery wire surface — the task name, the
//! `.delay()` payload constructor, positional-or-keyword arg binding —
//! and every pure task decision: the export filenames, the S3 key,
//! the `ExporterHistory` status SQL and the issue-filter SQL. The
//! serialisation engines live in `pidash-services`
//! (`app_analytics::export_format`); ZIP bytes, CSV sanitising, the S3
//! branch protocol, the upload row update and the provider check live in
//! [`pidash_services::tasks_cleanup::exports`] and the shared wire
//! constants in `pidash-types` — all reused here, never re-implemented.
//!
//! Ownership: this task stays Python-owned. No local handler is
//! registered here — registering one would steal live traffic from the
//! Python workers while the S3 upload, the presigned-URL minting and the
//! ORM prefetch cascade still live there. [`is_export_task`] is the
//! routing predicate the worker consults; the domain gate flips
//! ownership after the oracle replay passes on both backends (the
//! `tasks_webhooks` precedent).
//!
//! Ack parity: the bare `@shared_task` carries no autoretry and every
//! Python control path ends in `return` (success, `ValueError` failure or
//! logged exception) — a bound payload always settles. Only an
//! unbindable payload raises (`TypeError` in Python), which the worker
//! settles into requeue-with-budget exactly like the F-09 mechanism does
//! for every handler.

use serde_json::{Map, Value};

use pidash_services::app_analytics::queries::ISSUE_OBJECTS_SCOPE;
use pidash_services::tasks_cleanup::exports as cleanup;
use pidash_types::tasks_cleanup::exports_dto as dto;

use crate::celery::CeleryTaskMessage;

/// `issue_export_task` (`export_task.py:127-128`, bare `@shared_task`).
pub use dto::ISSUE_EXPORT_TASK_NAME as ISSUE_EXPORT_TASK;

/// Presigned-URL lifetime (`export_task.py:47`, 7 days in seconds).
pub use dto::EXPORT_URL_EXPIRES_IN_SECS as EXPORT_EXPIRES_IN_SECS;

/// `ExporterHistory` status after each task step.
pub const STATUS_PROCESSING: &str = "processing";
/// Set by [`cleanup::resolve_upload_update`] on a truthy presigned URL.
pub const STATUS_COMPLETED: &str = "completed";
/// Set on `ValueError` and on any other exception.
pub const STATUS_FAILED: &str = "failed";

/// `save(update_fields=[...])` column lists per step.
pub const UPDATE_FIELDS_PROCESSING: &[&str] = &["status"];
/// `export_task.py:200` (bad provider) and `:222-224` (exception).
pub const UPDATE_FIELDS_FAILED: &[&str] = &["status", "reason"];
/// `export_task.py:124` (upload write).
pub const UPDATE_FIELDS_UPLOADED: &[&str] = &["status", "url", "key"];

/// `select_related` chain of the task queryset (`export_task.py:156-162`).
pub const TASK_SELECT_RELATED: &[&str] = &[
    "project",
    "workspace",
    "state",
    "created_by",
    "estimate_point",
];

/// `prefetch_related` chain (`export_task.py:163-189`): plain relations
/// plus the four `Prefetch` querysets with their inner selects/ordering.
pub const TASK_PREFETCH_RELATED: &[&str] = &[
    "labels",
    "issue_cycle__cycle",
    "issue_module__module",
    "assignees",
    "issue_link",
    "issue_subscribers(subscriber)",
    "issue_comments(actor, order created_at)",
    "issue_relation(related_issue, related_issue__project)",
    "issue_related(issue, issue__project)",
    "parent(type, project)",
];

/// Bound `issue_export_task` arguments, in signature order
/// (`export_task.py:128-135`).
#[derive(Debug, Clone, PartialEq)]
pub struct ExportTaskCall {
    pub provider: String,
    pub workspace_id: String,
    pub project_ids: Vec<String>,
    pub token_id: String,
    pub multiple: bool,
    pub slug: String,
}

/// Routing predicate: true while this task name is Python-owned.
pub fn is_export_task(name: &str) -> bool {
    name == ISSUE_EXPORT_TASK
}

fn task_kwarg<'v>(kwargs: &'v Map<String, Value>, key: &str) -> Option<&'v Value> {
    kwargs.get(key)
}

fn as_text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// Bind one task argument: the kwarg wins when the key is present (even
/// when null, mirroring Python binding), otherwise the positional. The
/// view's `.delay()` call passes everything as keywords
/// (`app/views/exporter/base.py:49-56`).
fn bind_arg(args: &[Value], kwargs: &Map<String, Value>, key: &str, pos: usize) -> Option<Value> {
    if let Some(value) = task_kwarg(kwargs, key) {
        return Some(value.clone());
    }
    args.get(pos).cloned()
}

/// Bind a Celery `(args, kwargs)` payload to [`ExportTaskCall`]. Shape
/// errors are the Python `TypeError` path (unbindable — the worker
/// requeues with budget); semantic failures (bad provider) bind fine and
/// resolve to `failed` + reason downstream.
pub fn bind_export_task_call(args: &Value, kwargs: &Value) -> Result<ExportTaskCall, String> {
    let args = args
        .as_array()
        .ok_or("issue_export_task: args must be a list")?;
    let kwargs = kwargs
        .as_object()
        .ok_or("issue_export_task: kwargs must be an object")?;
    let need = |key: &str, pos: usize| {
        bind_arg(args, kwargs, key, pos)
            .ok_or_else(|| format!("issue_export_task: missing argument '{key}'"))
    };
    let provider = need("provider", 0)?;
    let workspace_id = need("workspace_id", 1)?;
    let project_ids = need("project_ids", 2)?;
    let token_id = need("token_id", 3)?;
    let multiple = need("multiple", 4)?;
    let slug = need("slug", 5)?;
    let provider = as_text(&provider).ok_or("issue_export_task: provider must be a string")?;
    let workspace_id =
        as_text(&workspace_id).ok_or("issue_export_task: workspace_id must be a string")?;
    let project_ids = project_ids
        .as_array()
        .ok_or("issue_export_task: project_ids must be a list")?
        .iter()
        .map(|v| as_text(v).ok_or("issue_export_task: project id must be a string"))
        .collect::<Result<Vec<_>, _>>()?;
    let token_id = as_text(&token_id).ok_or("issue_export_task: token_id must be a string")?;
    let multiple = multiple
        .as_bool()
        .ok_or("issue_export_task: multiple must be a boolean")?;
    let slug = as_text(&slug).ok_or("issue_export_task: slug must be a string")?;
    Ok(ExportTaskCall {
        provider,
        workspace_id,
        project_ids,
        token_id,
        multiple,
        slug,
    })
}

/// `issue_export_task.delay(...)` exactly as the exporter view calls it
/// (`app/views/exporter/base.py:49-56`): kwargs in signature order.
pub fn issue_export_task_message(call: &ExportTaskCall) -> CeleryTaskMessage {
    let mut kwargs = Map::new();
    kwargs.insert("provider".to_owned(), Value::String(call.provider.clone()));
    kwargs.insert(
        "workspace_id".to_owned(),
        Value::String(call.workspace_id.clone()),
    );
    kwargs.insert(
        "project_ids".to_owned(),
        Value::Array(
            call.project_ids
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ),
    );
    kwargs.insert("token_id".to_owned(), Value::String(call.token_id.clone()));
    kwargs.insert("multiple".to_owned(), Value::Bool(call.multiple));
    kwargs.insert("slug".to_owned(), Value::String(call.slug.clone()));
    CeleryTaskMessage::new(ISSUE_EXPORT_TASK, Vec::new(), kwargs)
}

/// `upload_to_s3` object key (`export_task.py:46`):
/// `{workspace_id}/export-{slug}-{token_id[:6]}-{today}.zip`.
pub fn export_s3_key(workspace_id: &str, slug: &str, token_id: &str, today_iso: &str) -> String {
    dto::export_s3_key(workspace_id, slug, token_id, today_iso)
}

/// Per-project vs single export filename (`export_task.py:204-215`).
pub fn task_export_filename(
    slug: &str,
    multiple: bool,
    project_id: &str,
    workspace_id: &str,
) -> String {
    dto::export_filename(slug, multiple, project_id, workspace_id)
}

/// Validate the task's `provider` (`export_task.py:193-201`): anything
/// outside csv/json/xlsx writes `failed` + the `ValueError` reason.
pub fn validate_task_provider(provider: &str) -> Result<(), String> {
    cleanup::validate_provider(provider)
}

/// `ExporterHistory.objects.get(token=token_id)` (`export_task.py:143`):
/// single-row fetch by the unique token (repo `LIMIT 1` convention for
/// `.get()` fetches).
pub fn fetch_exporter_by_token_sql(token_ph: &str) -> String {
    format!(
        "SELECT \"exporters\".* FROM \"exporters\" WHERE \"exporters\".\"token\" = {token_ph} LIMIT 1"
    )
}

/// `status = "processing"` write (`export_task.py:144-145`).
pub fn mark_processing_sql(status_ph: &str, id_ph: &str) -> String {
    format!(
        "UPDATE \"exporters\" SET \"status\" = {status_ph} WHERE \"exporters\".\"id\" = {id_ph}"
    )
}

/// `status = "failed"`, `reason = str(e)` write (`export_task.py:197-200`
/// for the `ValueError` branch, `:221-224` for the outer `except`).
pub fn mark_failed_sql(status_ph: &str, reason_ph: &str, id_ph: &str) -> String {
    format!(
        "UPDATE \"exporters\" SET \"status\" = {status_ph}, \"reason\" = {reason_ph} \
         WHERE \"exporters\".\"id\" = {id_ph}"
    )
}

/// Upload write (`export_task.py:114-124`): `url` + `completed` + `key`
/// on a truthy presigned URL, else `failed` (see
/// [`cleanup::resolve_upload_update`]).
pub fn mark_uploaded_sql(status_ph: &str, url_ph: &str, key_ph: &str, id_ph: &str) -> String {
    format!(
        "UPDATE \"exporters\" SET \"status\" = {status_ph}, \"url\" = {url_ph}, \"key\" = {key_ph} \
         WHERE \"exporters\".\"id\" = {id_ph}"
    )
}

/// The task's base issue queryset (`export_task.py:148-155`):
/// workspace + `project_id__in` + requesting-member active + project
/// unarchived, over the `Issue.issue_objects` manager scope (triage,
/// archived, project-archived and draft exclusions plus soft-delete —
/// [`ISSUE_OBJECTS_SCOPE`], `db/models/issue.py:95-104`). `project_phs`
/// is the caller's `project_id__in` placeholder list.
pub fn export_issues_filter_sql(
    workspace_ph: &str,
    project_phs: &[String],
    member_ph: &str,
) -> String {
    format!(
        "SELECT \"issues\".* FROM \"issues\" \
         INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") \
         INNER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") \
         INNER JOIN \"project_members\" ON (\"projects\".\"id\" = \"project_members\".\"project_id\") \
         WHERE (\"issues\".\"workspace_id\" = {workspace_ph} \
         AND \"issues\".\"project_id\" IN ({}) \
         AND \"project_members\".\"member_id\" = {member_ph} \
         AND \"project_members\".\"is_active\" \
         AND \"projects\".\"archived_at\" IS NULL \
         AND {ISSUE_OBJECTS_SCOPE})",
        project_phs.join(", ")
    )
}

/// The `multiple` per-project narrowing (`export_task.py:206-207`):
/// `workspace_issues.filter(project_id=project_id)`.
pub fn export_project_narrow_clause(project_ph: &str) -> String {
    format!("\"issues\".\"project_id\" = {project_ph}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call() -> ExportTaskCall {
        ExportTaskCall {
            provider: "csv".to_owned(),
            workspace_id: "ws-1".to_owned(),
            project_ids: vec!["p-1".to_owned(), "p-2".to_owned()],
            token_id: "abcdef123456".to_owned(),
            multiple: false,
            slug: "an".to_owned(),
        }
    }

    #[test]
    fn task_name_matches_celery_wire() {
        // Bare `@shared_task` default: module path + function name.
        assert_eq!(
            ISSUE_EXPORT_TASK,
            "pi_dash.bgtasks.export_task.issue_export_task"
        );
        assert!(is_export_task(ISSUE_EXPORT_TASK));
        assert!(!is_export_task("pi_dash.bgtasks.export_task.other"));
    }

    #[test]
    fn bind_prefers_kwargs_then_positionals() {
        let c = call();
        let msg = issue_export_task_message(&c);
        // Kwarg names ride the wire in signature order.
        let names: Vec<&str> = msg.kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            vec![
                "provider",
                "workspace_id",
                "project_ids",
                "token_id",
                "multiple",
                "slug"
            ]
        );
        // The v2 body carries the same pair as `[args, kwargs, embed]`.
        assert_eq!(msg.body()[1], Value::Object(msg.kwargs.clone()));
        let bound = bind_export_task_call(
            &Value::Array(msg.args.clone()),
            &Value::Object(msg.kwargs.clone()),
        )
        .expect("binds");
        assert_eq!(bound, c);

        // Positional form binds identically.
        let args = Value::Array(vec![
            Value::String("json".to_owned()),
            Value::String("ws-9".to_owned()),
            Value::Array(vec![Value::String("p-9".to_owned())]),
            Value::String("tok".to_owned()),
            Value::Bool(true),
            Value::String("s".to_owned()),
        ]);
        let bound = bind_export_task_call(&args, &Value::Object(Map::new())).expect("binds");
        assert_eq!(bound.provider, "json");
        assert_eq!(bound.workspace_id, "ws-9");
        assert!(bound.multiple);

        // Missing and mistyped arguments fail to bind (the TypeError path).
        assert!(bind_export_task_call(&args, &Value::Null).is_err());
        assert!(bind_export_task_call(&Value::Null, &Value::Object(Map::new())).is_err());
        let mut bad = Map::new();
        bad.insert("provider".to_owned(), Value::Null);
        assert!(bind_export_task_call(&Value::Array(vec![]), &Value::Object(bad)).is_err());
    }

    #[test]
    fn filenames_and_key_match_fixture() {
        // FX-A-T-02b `file_name` golden.
        assert_eq!(
            export_s3_key("ws-1", "an", "abcdef123456", "2026-09-28"),
            "ws-1/export-an-abcdef-2026-09-28.zip"
        );
        assert_eq!(EXPORT_EXPIRES_IN_SECS, 604800);
        // FX-A-T-01 `multiple` filenames.
        assert_eq!(task_export_filename("an", true, "p-1", "ws-1"), "an-p-1");
        assert_eq!(task_export_filename("an", false, "p-1", "ws-1"), "an-ws-1");
        // Bad providers fail with the exact ValueError reason.
        assert_eq!(
            validate_task_provider("xml"),
            Err("Unsupported format: xml. Available: ['csv', 'json', 'xlsx']".to_owned())
        );
        assert!(validate_task_provider("xlsx").is_ok());
    }

    #[test]
    fn status_sql_pins_columns() {
        assert_eq!(
            fetch_exporter_by_token_sql("$1"),
            "SELECT \"exporters\".* FROM \"exporters\" WHERE \"exporters\".\"token\" = $1 LIMIT 1"
        );
        assert_eq!(
            mark_processing_sql("$1", "$2"),
            "UPDATE \"exporters\" SET \"status\" = $1 WHERE \"exporters\".\"id\" = $2"
        );
        assert_eq!(
            mark_failed_sql("$1", "$2", "$3"),
            "UPDATE \"exporters\" SET \"status\" = $1, \"reason\" = $2 WHERE \"exporters\".\"id\" = $3"
        );
        assert_eq!(
            mark_uploaded_sql("$1", "$2", "$3", "$4"),
            "UPDATE \"exporters\" SET \"status\" = $1, \"url\" = $2, \"key\" = $3 WHERE \"exporters\".\"id\" = $4"
        );
        assert_eq!(UPDATE_FIELDS_PROCESSING, &["status"]);
        assert_eq!(UPDATE_FIELDS_FAILED, &["status", "reason"]);
        assert_eq!(UPDATE_FIELDS_UPLOADED, &["status", "url", "key"]);
    }

    #[test]
    fn issue_filter_sql_pins_guards() {
        let sql = export_issues_filter_sql("$1", &["$2".to_owned(), "$3".to_owned()], "$4");
        assert!(sql.contains("\"issues\".\"workspace_id\" = $1"));
        assert!(sql.contains("\"issues\".\"project_id\" IN ($2, $3)"));
        assert!(sql.contains("\"project_members\".\"member_id\" = $4"));
        assert!(sql.contains("\"project_members\".\"is_active\""));
        assert!(sql.contains("\"projects\".\"archived_at\" IS NULL"));
        // The IssueManager exclusions ride along verbatim.
        assert!(sql.contains("\"states\".\"group\" != 'triage'"));
        assert!(sql.contains("\"issues\".\"is_draft\" = FALSE"));
        assert_eq!(
            export_project_narrow_clause("$1"),
            "\"issues\".\"project_id\" = $1"
        );
        // The ORM chains are recorded, not re-ported.
        assert_eq!(
            TASK_SELECT_RELATED,
            &[
                "project",
                "workspace",
                "state",
                "created_by",
                "estimate_point"
            ]
        );
        assert!(TASK_PREFETCH_RELATED.contains(&"issue_comments(actor, order created_at)"));
    }

    #[test]
    fn s3_protocol_reuses_shared_kernels() {
        // MinIO uploads carry the public-read ACL; both branches presign
        // with a 7-day expiry and write the same row update.
        assert_eq!(
            cleanup::select_s3_branch(true, ""),
            cleanup::S3Branch::Minio
        );
        assert_eq!(
            cleanup::upload_extra_args(cleanup::S3Branch::Minio),
            vec![("ACL", "public-read"), ("ContentType", "application/zip")]
        );
        let done = cleanup::resolve_upload_update(Some("https://cdn/x.zip"), "k");
        assert_eq!(done.status, STATUS_COMPLETED);
        assert_eq!(done.url.as_deref(), Some("https://cdn/x.zip"));
        let failed = cleanup::resolve_upload_update(None, "k");
        assert_eq!(failed.status, STATUS_FAILED);
    }
}
