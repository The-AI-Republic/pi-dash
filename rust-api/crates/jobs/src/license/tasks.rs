//! `instance_traces` background task (D-01, stage 3).
//!
//! Port of `apps/api/pi_dash/license/bgtasks/tracer.py:26-105`
//! (`@shared_task`, beat entry `run-every-6-hours-for-instance-trace` in
//! `apps/api/pi_dash/celery.py:34-37`: `crontab(hour="*/6", minute=0)`).
//!
//! The task has no arguments and returns `None` implicitly on every path.
//! Span construction is pure ([`build_spans`]) over injected snapshots so
//! the recorded vectors replay without a database or an exporter; the
//! async [`run`] drives it against Postgres + a [`SpanSink`], and
//! [`register`] installs the worker [`Registry`][crate::worker::Registry]
//! handler that owns the Celery wire name. [`delay_message`] is the
//! Celery-protocol-v2 `.delay()` equivalent used by `register_instance`'s
//! tail and by anything else that must enqueue this task during
//! coexistence.
//!
//! # Ported semantics (translate, don't redesign)
//!
//! * instance-None → return with no spans (`tracer.py:31-35`); the shutdown
//!   in `finally` (`:103-105`) still runs.
//! * telemetry-off → both span blocks skipped, `return None` (`:37,102`);
//!   shutdown still runs.
//! * `instance_details` carries exactly 21 attributes in source order
//!   (`:45-74`); `latest_version` is set verbatim even when `None`
//!   (`:57`), hence [`AttributeValue::Null`].
//! * one `workspace_details` span per workspace (`:77-100`), 11 attributes
//!   each; per-workspace counts filter by that workspace, and
//!   `member_count` counts `WorkspaceMember` rows with NO `is_bot` /
//!   `is_active` filter — unlike the `workspace_list` member annotation.
//! * `workspace_id` is `str(workspace.id)` (`:88`).
//! * `init_tracer()` runs before anything else (`:29`); the OTLP exporter
//!   it installs is process-global runtime state, so the exporter half
//!   stays in worker boot — this module only decides *which* spans with
//!   *which* attributes and guarantees the shutdown call on every path.
//! * counts use the default managers: soft-delete-filtered
//!   (`deleted_at IS NULL`) for every model except `users`, which has no
//!   soft-delete manager (Django's `UserManager`).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Map;

use crate::celery::CeleryTaskMessage;
use crate::worker::{Handler, Registry, Verdict};

/// Celery wire name, exactly as the beat schedule and `.delay()` call it.
pub const TASK_NAME: &str = "pi_dash.license.bgtasks.tracer.instance_traces";

/// `instance_details` attribute order, `tracer.py:45-74`.
pub const INSTANCE_DETAILS_ATTRS: [&str; 21] = [
    "instance_id",
    "instance_name",
    "current_version",
    "latest_version",
    "is_telemetry_enabled",
    "is_support_required",
    "is_setup_done",
    "is_signup_screen_visited",
    "is_verified",
    "edition",
    "domain",
    "is_test",
    "user_count",
    "workspace_count",
    "project_count",
    "issue_count",
    "module_count",
    "cycle_count",
    "cycle_issue_count",
    "module_issue_count",
    "page_count",
];

/// `workspace_details` attribute order, `tracer.py:84-98`.
pub const WORKSPACE_DETAILS_ATTRS: [&str; 11] = [
    "instance_id",
    "workspace_id",
    "workspace_slug",
    "project_count",
    "issue_count",
    "module_count",
    "cycle_count",
    "cycle_issue_count",
    "module_issue_count",
    "page_count",
    "member_count",
];

/// One span attribute value. `Null` exists only because `latest_version`
/// is set verbatim even when the column is `NULL` (`tracer.py:57`).
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeValue {
    Str(String),
    Int(i64),
    Bool(bool),
    Null,
}

/// One emitted span: its name plus attributes in emission order.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub name: &'static str,
    pub attributes: Vec<(&'static str, AttributeValue)>,
}

/// The `Instance` columns the task reads (the 12 instance attributes of
/// `instance_details`; `tracer.py:45-56`).
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceSnapshot {
    pub instance_id: String,
    pub instance_name: String,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub is_telemetry_enabled: bool,
    pub is_support_required: bool,
    pub is_setup_done: bool,
    pub is_signup_screen_visited: bool,
    pub is_verified: bool,
    pub edition: String,
    pub domain: String,
    pub is_test: bool,
}

/// The nine global `.count()` calls (`tracer.py:43-51`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GlobalCounts {
    pub user_count: i64,
    pub workspace_count: i64,
    pub project_count: i64,
    pub issue_count: i64,
    pub module_count: i64,
    pub cycle_count: i64,
    pub cycle_issue_count: i64,
    pub module_issue_count: i64,
    pub page_count: i64,
}

/// One workspace plus its eight per-workspace `.filter(workspace=…).count()`
/// calls (`tracer.py:77-100`).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceInput {
    /// Already `str(workspace.id)` (`tracer.py:88`).
    pub workspace_id: String,
    pub workspace_slug: String,
    pub project_count: i64,
    pub issue_count: i64,
    pub module_count: i64,
    pub cycle_count: i64,
    pub cycle_issue_count: i64,
    pub module_issue_count: i64,
    pub page_count: i64,
    pub member_count: i64,
}

/// Build the task's spans: `None` instance or disabled telemetry yields no
/// spans (`tracer.py:31-37`); otherwise one `instance_details` span plus one
/// `workspace_details` span per workspace, in iteration order.
pub fn build_spans(
    instance: Option<&InstanceSnapshot>,
    global: &GlobalCounts,
    workspaces: &[WorkspaceInput],
) -> Vec<Span> {
    let instance = match instance {
        None => return Vec::new(),
        Some(instance) => instance,
    };
    if !instance.is_telemetry_enabled {
        return Vec::new();
    }
    let mut spans = Vec::with_capacity(1 + workspaces.len());
    spans.push(Span {
        name: "instance_details",
        attributes: vec![
            (
                "instance_id",
                AttributeValue::Str(instance.instance_id.clone()),
            ),
            (
                "instance_name",
                AttributeValue::Str(instance.instance_name.clone()),
            ),
            (
                "current_version",
                AttributeValue::Str(instance.current_version.clone()),
            ),
            (
                "latest_version",
                match &instance.latest_version {
                    Some(v) => AttributeValue::Str(v.clone()),
                    None => AttributeValue::Null,
                },
            ),
            (
                "is_telemetry_enabled",
                AttributeValue::Bool(instance.is_telemetry_enabled),
            ),
            (
                "is_support_required",
                AttributeValue::Bool(instance.is_support_required),
            ),
            (
                "is_setup_done",
                AttributeValue::Bool(instance.is_setup_done),
            ),
            (
                "is_signup_screen_visited",
                AttributeValue::Bool(instance.is_signup_screen_visited),
            ),
            ("is_verified", AttributeValue::Bool(instance.is_verified)),
            ("edition", AttributeValue::Str(instance.edition.clone())),
            ("domain", AttributeValue::Str(instance.domain.clone())),
            ("is_test", AttributeValue::Bool(instance.is_test)),
            ("user_count", AttributeValue::Int(global.user_count)),
            (
                "workspace_count",
                AttributeValue::Int(global.workspace_count),
            ),
            ("project_count", AttributeValue::Int(global.project_count)),
            ("issue_count", AttributeValue::Int(global.issue_count)),
            ("module_count", AttributeValue::Int(global.module_count)),
            ("cycle_count", AttributeValue::Int(global.cycle_count)),
            (
                "cycle_issue_count",
                AttributeValue::Int(global.cycle_issue_count),
            ),
            (
                "module_issue_count",
                AttributeValue::Int(global.module_issue_count),
            ),
            ("page_count", AttributeValue::Int(global.page_count)),
        ],
    });
    for workspace in workspaces {
        spans.push(Span {
            name: "workspace_details",
            attributes: vec![
                (
                    "instance_id",
                    AttributeValue::Str(instance.instance_id.clone()),
                ),
                (
                    "workspace_id",
                    AttributeValue::Str(workspace.workspace_id.clone()),
                ),
                (
                    "workspace_slug",
                    AttributeValue::Str(workspace.workspace_slug.clone()),
                ),
                (
                    "project_count",
                    AttributeValue::Int(workspace.project_count),
                ),
                ("issue_count", AttributeValue::Int(workspace.issue_count)),
                ("module_count", AttributeValue::Int(workspace.module_count)),
                ("cycle_count", AttributeValue::Int(workspace.cycle_count)),
                (
                    "cycle_issue_count",
                    AttributeValue::Int(workspace.cycle_issue_count),
                ),
                (
                    "module_issue_count",
                    AttributeValue::Int(workspace.module_issue_count),
                ),
                ("page_count", AttributeValue::Int(workspace.page_count)),
                ("member_count", AttributeValue::Int(workspace.member_count)),
            ],
        });
    }
    spans
}

/// Where spans go. `shutdown` is the `shutdown_tracer()` half of
/// `tracer.py:103-105`; [`run`] calls it on every path, including the
/// early returns and errors, mirroring the `finally`.
pub trait SpanSink {
    fn emit(&mut self, span: Span);
    fn shutdown(&mut self);
}

/// Where the snapshots come from. One method per query family so fakes
/// stay trivial and the Postgres implementation stays the only SQL owner.
pub trait TraceDb {
    /// `Instance.objects.first()` — newest non-deleted row, if any.
    fn instance_first(
        &self,
    ) -> impl std::future::Future<Output = Result<Option<InstanceSnapshot>, String>> + Send;
    /// The nine global counts, `tracer.py:43-51`.
    fn global_counts(
        &self,
    ) -> impl std::future::Future<Output = Result<GlobalCounts, String>> + Send;
    /// Every non-deleted workspace with its eight filtered counts.
    fn workspace_inputs(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<WorkspaceInput>, String>> + Send;
}

/// Run the task: fetch, build, emit, and always shut down — the `return`
/// value mirrors the implicit `None`, and the shutdown runs even when a
/// fetch fails (the `finally` at `tracer.py:103-105`).
pub async fn run<D: TraceDb, S: SpanSink>(db: &D, sink: &mut S) -> Result<(), String> {
    let outcome = run_inner(db, sink).await;
    sink.shutdown();
    outcome
}

async fn run_inner<D: TraceDb, S: SpanSink>(db: &D, sink: &mut S) -> Result<(), String> {
    let instance = db.instance_first().await?;
    let instance = match instance {
        None => return Ok(()),
        Some(instance) => instance,
    };
    if !instance.is_telemetry_enabled {
        return Ok(());
    }
    // The global counts are read before the workspace loop, in source
    // order (`tracer.py:43-51` ahead of `:77`); the per-workspace counts
    // arrive with their workspace rows.
    let global = db.global_counts().await?;
    let workspaces = db.workspace_inputs().await?;
    for span in build_spans(Some(&instance), &global, &workspaces) {
        sink.emit(span);
    }
    Ok(())
}

/// The `.delay()` equivalent: a first-attempt Celery protocol v2 message
/// for this task with no args, exactly what `instance_traces.delay()`
/// publishes (`register_instance.py:90`, beat dispatch the same shape).
pub fn delay_message() -> CeleryTaskMessage {
    CeleryTaskMessage::new(TASK_NAME, Vec::new(), Map::new())
}

/// Install the worker handler owning [`TASK_NAME`]. A store failure
/// requeues with the Celery default retry delay (the bare `@shared_task`
/// carries `max_retries = 3`, `default_retry_delay = 180s` per the jobs
/// plane); the row parks as failed once the budget is spent.
pub fn register(registry: &mut Registry, pool: sqlx::PgPool) {
    let handler: Handler = Arc::new(move |_job: crate::queue::JobRow| {
        let pool = pool.clone();
        Box::pin(async move {
            let db = PgTraceDb { pool };
            let mut sink = TracingSink;
            // A store failure propagates as a handler error: the worker
            // requeues with `DEFAULT_RETRY_DELAY_SECS` while the Celery
            // retry budget (`max_retries = 3`) lasts, then parks the row
            // as failed with this text.
            match run(&db, &mut sink).await {
                Ok(()) => Ok(Verdict::Ack),
                Err(error) => Err(error),
            }
        })
    });
    registry.register(TASK_NAME, handler);
}

/// [`SpanSink`] that emits one structured event per span. The OTLP exporter
/// itself is worker-boot state (like Python's process-global provider from
/// `init_tracer`); these events are the local half the exporter scrapes.
struct TracingSink;

impl SpanSink for TracingSink {
    fn emit(&mut self, span: Span) {
        let attrs: HashMap<&str, String> = span
            .attributes
            .iter()
            .map(|(key, value)| (*key, render_attribute(value)))
            .collect();
        tracing::info!(otel.span = span.name, otel.attributes = ?attrs, "trace span");
    }

    fn shutdown(&mut self) {}
}

fn render_attribute(value: &AttributeValue) -> String {
    match value {
        AttributeValue::Str(s) => s.clone(),
        AttributeValue::Int(n) => n.to_string(),
        AttributeValue::Bool(b) => b.to_string(),
        AttributeValue::Null => String::from("null"),
    }
}

/// Postgres [`TraceDb`]: the exact Django query shapes.
pub struct PgTraceDb {
    pool: sqlx::PgPool,
}

impl PgTraceDb {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// `Model.objects.count()` over `table`, soft-delete-aware except for
    /// `users` (no soft-delete manager there).
    async fn count(
        pool: &sqlx::PgPool,
        table: &str,
        soft_deleted: bool,
    ) -> Result<i64, sqlx::Error> {
        let sql = if soft_deleted {
            format!("SELECT COUNT(*) FROM {table} WHERE deleted_at IS NULL")
        } else {
            format!("SELECT COUNT(*) FROM {table}")
        };
        sqlx::query_scalar(&sql).fetch_one(pool).await
    }

    /// `Model.objects.filter(workspace = …).count()`: every per-workspace
    /// table here is soft-delete-filtered through the default manager.
    /// `table` is always one of the eight literal names at the call sites
    /// below, never caller input.
    async fn count_for_workspace(
        pool: &sqlx::PgPool,
        table: &'static str,
        workspace_id: uuid::Uuid,
    ) -> Result<i64, sqlx::Error> {
        let sql =
            format!("SELECT COUNT(*) FROM {table} WHERE workspace_id = $1 AND deleted_at IS NULL");
        sqlx::query_scalar(&sql)
            .bind(workspace_id)
            .fetch_one(pool)
            .await
    }
}

impl TraceDb for PgTraceDb {
    async fn instance_first(&self) -> Result<Option<InstanceSnapshot>, String> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_db::license::queries::INSTANCE_FIRST_SQL)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
        row.map(|row| {
            pidash_db::license::queries::map_instance_row(&row)
                .map(InstanceSnapshot::from)
                .map_err(|e| e.to_string())
        })
        .transpose()
    }

    async fn global_counts(&self) -> Result<GlobalCounts, String> {
        // Table order follows the source order at `tracer.py:43-51`.
        let workspace_count = Self::count(&self.pool, "workspaces", true)
            .await
            .map_err(|e| e.to_string())?;
        let user_count = Self::count(&self.pool, "users", false)
            .await
            .map_err(|e| e.to_string())?;
        let project_count = Self::count(&self.pool, "projects", true)
            .await
            .map_err(|e| e.to_string())?;
        let issue_count = Self::count(&self.pool, "issues", true)
            .await
            .map_err(|e| e.to_string())?;
        let module_count = Self::count(&self.pool, "modules", true)
            .await
            .map_err(|e| e.to_string())?;
        let cycle_count = Self::count(&self.pool, "cycles", true)
            .await
            .map_err(|e| e.to_string())?;
        let cycle_issue_count = Self::count(&self.pool, "cycle_issues", true)
            .await
            .map_err(|e| e.to_string())?;
        let module_issue_count = Self::count(&self.pool, "module_issues", true)
            .await
            .map_err(|e| e.to_string())?;
        let page_count = Self::count(&self.pool, "pages", true)
            .await
            .map_err(|e| e.to_string())?;
        Ok(GlobalCounts {
            user_count,
            workspace_count,
            project_count,
            issue_count,
            module_count,
            cycle_count,
            cycle_issue_count,
            module_issue_count,
            page_count,
        })
    }

    async fn workspace_inputs(&self) -> Result<Vec<WorkspaceInput>, String> {
        let rows: Vec<(uuid::Uuid, String)> =
            // `Workspace.objects.all()`: default manager scope plus the
            // `Meta.ordering = ("-created_at",)` span-emission order.
            sqlx::query_as(
                "SELECT id, slug FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at DESC",
            )
                .fetch_all(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
        let mut inputs = Vec::with_capacity(rows.len());
        for (id, slug) in rows {
            inputs.push(WorkspaceInput {
                workspace_id: id.to_string(),
                workspace_slug: slug,
                project_count: Self::count_for_workspace(&self.pool, "projects", id)
                    .await
                    .map_err(|e| e.to_string())?,
                issue_count: Self::count_for_workspace(&self.pool, "issues", id)
                    .await
                    .map_err(|e| e.to_string())?,
                module_count: Self::count_for_workspace(&self.pool, "modules", id)
                    .await
                    .map_err(|e| e.to_string())?,
                cycle_count: Self::count_for_workspace(&self.pool, "cycles", id)
                    .await
                    .map_err(|e| e.to_string())?,
                cycle_issue_count: Self::count_for_workspace(&self.pool, "cycle_issues", id)
                    .await
                    .map_err(|e| e.to_string())?,
                module_issue_count: Self::count_for_workspace(&self.pool, "module_issues", id)
                    .await
                    .map_err(|e| e.to_string())?,
                page_count: Self::count_for_workspace(&self.pool, "pages", id)
                    .await
                    .map_err(|e| e.to_string())?,
                member_count: Self::count_for_workspace(&self.pool, "workspace_members", id)
                    .await
                    .map_err(|e| e.to_string())?,
            });
        }
        Ok(inputs)
    }
}

impl From<pidash_db::license::models::instance::Instance> for InstanceSnapshot {
    fn from(row: pidash_db::license::models::instance::Instance) -> Self {
        Self {
            instance_id: row.instance_id,
            instance_name: row.instance_name,
            current_version: row.current_version,
            latest_version: row.latest_version,
            is_telemetry_enabled: row.is_telemetry_enabled,
            is_support_required: row.is_support_required,
            is_setup_done: row.is_setup_done,
            is_signup_screen_visited: row.is_signup_screen_visited,
            is_verified: row.is_verified,
            edition: row.edition,
            domain: row.domain,
            is_test: row.is_test,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/license/tasks")
            .join(name)
    }

    fn fixture() -> serde_json::Value {
        let text = std::fs::read_to_string(fixture_path("instance_traces.before_after.json"))
            .expect("task fixture exists");
        serde_json::from_str(&text).expect("task fixture is valid JSON")
    }

    fn example_instance() -> InstanceSnapshot {
        InstanceSnapshot {
            instance_id: "abc123".to_string(),
            instance_name: "Acme".to_string(),
            current_version: "v1.0.0".to_string(),
            latest_version: Some("v1.0.1".to_string()),
            is_telemetry_enabled: true,
            is_support_required: false,
            is_setup_done: true,
            is_signup_screen_visited: true,
            is_verified: true,
            edition: "PI_DASH_COMMUNITY".to_string(),
            domain: "acme.test".to_string(),
            is_test: false,
        }
    }

    fn example_global() -> GlobalCounts {
        GlobalCounts {
            user_count: 7,
            workspace_count: 2,
            project_count: 5,
            issue_count: 12,
            module_count: 2,
            cycle_count: 1,
            cycle_issue_count: 2,
            module_issue_count: 3,
            page_count: 4,
        }
    }

    fn example_workspace() -> WorkspaceInput {
        WorkspaceInput {
            workspace_id: "55555555-5555-5555-5555-555555555555".to_string(),
            workspace_slug: "acme-works".to_string(),
            project_count: 5,
            issue_count: 12,
            module_count: 0,
            cycle_count: 0,
            cycle_issue_count: 0,
            module_issue_count: 0,
            page_count: 0,
            member_count: 7,
        }
    }

    fn attr_names(span: &Span) -> Vec<&str> {
        span.attributes.iter().map(|(key, _)| *key).collect()
    }

    #[test]
    fn wire_name_matches_beat_and_delay_call() {
        assert_eq!(TASK_NAME, "pi_dash.license.bgtasks.tracer.instance_traces");
    }

    #[test]
    fn instance_details_carries_21_attributes_in_fixture_order() {
        let recorded: Vec<String> = fixture()["instance_details_span"]["attributes"]
            .as_array()
            .expect("attributes list")
            .iter()
            .map(|v| v.as_str().expect("attr name").to_string())
            .collect();
        let emitted: Vec<String> = INSTANCE_DETAILS_ATTRS
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(emitted, recorded);
        let spans = build_spans(Some(&example_instance()), &example_global(), &[]);
        assert_eq!(spans.len(), 1);
        assert_eq!(
            attr_names(&spans[0]).as_slice(),
            INSTANCE_DETAILS_ATTRS.as_slice()
        );
    }

    #[test]
    fn workspace_details_carries_11_attributes_in_fixture_order() {
        let recorded: Vec<String> = fixture()["workspace_details_span"]["attributes"]
            .as_array()
            .expect("attributes list")
            .iter()
            .map(|v| {
                // The fixture annotates `workspace_id (str(workspace.id))`;
                // the attribute name itself is `workspace_id`.
                let name = v.as_str().expect("attr name");
                name.split_whitespace().next().expect("name").to_string()
            })
            .collect();
        let emitted: Vec<String> = WORKSPACE_DETAILS_ATTRS
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(emitted, recorded);
        let spans = build_spans(
            Some(&example_instance()),
            &example_global(),
            &[example_workspace()],
        );
        assert_eq!(spans.len(), 2);
        assert_eq!(
            attr_names(&spans[1]).as_slice(),
            WORKSPACE_DETAILS_ATTRS.as_slice()
        );
    }

    #[test]
    fn instance_details_example_values_match_fixture() {
        let example = &fixture()["instance_details_span"]["example"];
        let spans = build_spans(Some(&example_instance()), &example_global(), &[]);
        let got: HashMap<&str, &AttributeValue> =
            spans[0].attributes.iter().map(|(k, v)| (*k, v)).collect();
        let str_attr = |key: &str| match got[key] {
            AttributeValue::Str(s) => s.clone(),
            other => panic!("{key} should be a string, got {other:?}"),
        };
        let int_attr = |key: &str| match got[key] {
            AttributeValue::Int(n) => *n,
            other => panic!("{key} should be an int, got {other:?}"),
        };
        let bool_attr = |key: &str| match got[key] {
            AttributeValue::Bool(b) => *b,
            other => panic!("{key} should be a bool, got {other:?}"),
        };
        assert_eq!(
            str_attr("instance_id"),
            example["instance_id"].as_str().unwrap()
        );
        assert_eq!(
            str_attr("instance_name"),
            example["instance_name"].as_str().unwrap()
        );
        assert_eq!(
            str_attr("current_version"),
            example["current_version"].as_str().unwrap()
        );
        assert_eq!(
            str_attr("latest_version"),
            example["latest_version"].as_str().unwrap()
        );
        assert_eq!(str_attr("edition"), example["edition"].as_str().unwrap());
        assert_eq!(str_attr("domain"), example["domain"].as_str().unwrap());
        assert_eq!(
            bool_attr("is_telemetry_enabled"),
            example["is_telemetry_enabled"].as_bool().unwrap()
        );
        assert_eq!(
            bool_attr("is_support_required"),
            example["is_support_required"].as_bool().unwrap()
        );
        assert_eq!(
            bool_attr("is_setup_done"),
            example["is_setup_done"].as_bool().unwrap()
        );
        assert_eq!(
            bool_attr("is_signup_screen_visited"),
            example["is_signup_screen_visited"].as_bool().unwrap()
        );
        assert_eq!(
            bool_attr("is_verified"),
            example["is_verified"].as_bool().unwrap()
        );
        assert_eq!(bool_attr("is_test"), example["is_test"].as_bool().unwrap());
        assert_eq!(
            int_attr("user_count"),
            example["user_count"].as_i64().unwrap()
        );
        assert_eq!(
            int_attr("workspace_count"),
            example["workspace_count"].as_i64().unwrap()
        );
        assert_eq!(
            int_attr("project_count"),
            example["project_count"].as_i64().unwrap()
        );
        assert_eq!(
            int_attr("issue_count"),
            example["issue_count"].as_i64().unwrap()
        );
        assert_eq!(
            int_attr("module_count"),
            example["module_count"].as_i64().unwrap()
        );
        assert_eq!(
            int_attr("cycle_count"),
            example["cycle_count"].as_i64().unwrap()
        );
        assert_eq!(
            int_attr("cycle_issue_count"),
            example["cycle_issue_count"].as_i64().unwrap()
        );
        assert_eq!(
            int_attr("module_issue_count"),
            example["module_issue_count"].as_i64().unwrap()
        );
        assert_eq!(
            int_attr("page_count"),
            example["page_count"].as_i64().unwrap()
        );
    }

    #[test]
    fn latest_version_none_stays_null_verbatim() {
        let mut instance = example_instance();
        instance.latest_version = None;
        let spans = build_spans(Some(&instance), &example_global(), &[]);
        let latest = spans[0]
            .attributes
            .iter()
            .find(|(key, _)| *key == "latest_version")
            .expect("latest_version attribute present")
            .1
            .clone();
        assert_eq!(latest, AttributeValue::Null);
    }

    #[test]
    fn instance_none_yields_no_spans() {
        assert!(build_spans(None, &example_global(), &[example_workspace()]).is_empty());
    }

    #[test]
    fn telemetry_disabled_yields_no_spans() {
        let mut instance = example_instance();
        instance.is_telemetry_enabled = false;
        assert!(build_spans(Some(&instance), &example_global(), &[example_workspace()]).is_empty());
    }

    #[test]
    fn one_span_per_workspace_in_order() {
        let mut second = example_workspace();
        second.workspace_slug = "second".to_string();
        let spans = build_spans(
            Some(&example_instance()),
            &example_global(),
            &[example_workspace(), second],
        );
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].name, "instance_details");
        assert_eq!(spans[1].name, "workspace_details");
        assert_eq!(spans[2].name, "workspace_details");
        let slug = |span: &Span| match span.attributes[2] {
            ("workspace_slug", AttributeValue::Str(ref s)) => s.clone(),
            ref other => panic!("unexpected third attribute: {other:?}"),
        };
        assert_eq!(slug(&spans[1]), "acme-works");
        assert_eq!(slug(&spans[2]), "second");
        // Every workspace span repeats the instance id (`tracer.py:87`).
        for span in &spans[1..] {
            assert_eq!(
                span.attributes[0],
                ("instance_id", AttributeValue::Str("abc123".to_string()))
            );
        }
    }

    struct RecordingDb {
        instance: Option<InstanceSnapshot>,
        fail: bool,
    }

    struct RecordingSink {
        spans: Vec<Span>,
        shutdowns: usize,
    }

    impl SpanSink for RecordingSink {
        fn emit(&mut self, span: Span) {
            self.spans.push(span);
        }

        fn shutdown(&mut self) {
            self.shutdowns += 1;
        }
    }

    impl TraceDb for RecordingDb {
        async fn instance_first(&self) -> Result<Option<InstanceSnapshot>, String> {
            if self.fail {
                return Err("db down".to_string());
            }
            Ok(self.instance.clone())
        }

        async fn global_counts(&self) -> Result<GlobalCounts, String> {
            Ok(example_global())
        }

        async fn workspace_inputs(&self) -> Result<Vec<WorkspaceInput>, String> {
            Ok(vec![example_workspace()])
        }
    }

    #[tokio::test]
    async fn shutdown_runs_on_every_path() {
        // Enabled: spans + shutdown.
        let db = RecordingDb {
            instance: Some(example_instance()),
            fail: false,
        };
        let mut sink = RecordingSink {
            spans: Vec::new(),
            shutdowns: 0,
        };
        run(&db, &mut sink).await.expect("run succeeds");
        assert_eq!(sink.spans.len(), 2);
        assert_eq!(sink.shutdowns, 1);

        // Instance-None: no spans, shutdown still runs (the `finally`).
        let db = RecordingDb {
            instance: None,
            fail: false,
        };
        let mut sink = RecordingSink {
            spans: Vec::new(),
            shutdowns: 0,
        };
        run(&db, &mut sink).await.expect("run succeeds");
        assert!(sink.spans.is_empty());
        assert_eq!(sink.shutdowns, 1);

        // Telemetry-off: no spans, shutdown still runs.
        let mut off = example_instance();
        off.is_telemetry_enabled = false;
        let db = RecordingDb {
            instance: Some(off),
            fail: false,
        };
        let mut sink = RecordingSink {
            spans: Vec::new(),
            shutdowns: 0,
        };
        run(&db, &mut sink).await.expect("run succeeds");
        assert!(sink.spans.is_empty());
        assert_eq!(sink.shutdowns, 1);

        // Store failure: error propagates AND shutdown still runs.
        let db = RecordingDb {
            instance: Some(example_instance()),
            fail: true,
        };
        let mut sink = RecordingSink {
            spans: Vec::new(),
            shutdowns: 0,
        };
        assert!(run(&db, &mut sink).await.is_err());
        assert_eq!(sink.shutdowns, 1);
    }

    #[test]
    fn delay_message_is_a_first_attempt_celery_message() {
        let message = delay_message();
        assert_eq!(message.task, TASK_NAME);
        assert!(message.args.is_empty());
        assert!(message.kwargs.is_empty());
        assert_eq!(message.retries, 0);
        assert_eq!(message.effective_root_id(), message.id);
    }
}
