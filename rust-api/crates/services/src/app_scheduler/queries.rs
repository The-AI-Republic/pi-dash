#![forbid(unsafe_code)]

//! Scheduler + binding query builders (D-36, stage 5).
//!
//! Ports the query + write halves of `app/views/scheduler/views.py:1-291`
//! as SQL text plus row structs, following the D-27/D-30 precedent
//! (`app_cycles/queries.rs`, `app_pages/queries.rs`): each item is a
//! fragment or statement the caller splices into what it executes.
//! Placeholders stay symbolic — `:slug`, `:project_id`, `:scheduler_id`,
//! `:binding_id`, `:workspace_id`, `:now`, `:user_id`, … — exactly the
//! notation the fixtures use. The services crate carries no `sqlx`
//! dependency, so handlers translate each `:name` to a positional `$n`
//! in [`*_PARAMS`] order when binding via `sqlx`.
//!
//! Sources (drift baseline `01a93e17`):
//! - `app/views/scheduler/views.py:46-157` — scheduler list/detail
//!   reads, create, PATCH, delete cascade.
//! - `app/views/scheduler/views.py:163-291` — binding list/detail reads,
//!   install, PATCH, uninstall.
//! - `app/serializers/scheduler.py:108` — `active_binding_count`
//!   fallback count (R6).
//! - `bgtasks/scheduler.py:70-83` — `_next_fire_for_binding`, ported as
//!   the injected-closure call shape (the expansion itself lives in the
//!   jobs crate, on which services must not depend).
//! - `db/models/scheduler.py:107-251` — tables, defaults, constraints
//!   (structs already ported at
//!   `pidash_db::tasks_ticker::models`, reused here, never re-ported).
//! - `runner/models.py:52-122,872-1040` — `pod` / `agent_run` columns
//!   (F36-06; table-level reads only via local row structs).
//!
//! Fixture oracles: F36-04 (`queries/scheduler_sql.sql` + `.rows.json`),
//! F36-05 (`queries/binding_sql.sql` + `.rows.json`), F36-06
//! (`queries/pod_lastrun_columns.json`). The unit tests below pin every
//! statement byte-for-byte against those files so transcription drift
//! fails the build.
//!
//! Write-shape note: the fixture UPDATEs record the semantic write set
//! (writable + audit columns). Django's `save()` additionally rewrites
//! the remaining columns with their identical in-memory values; that is
//! unobservable (no triggers on these tables) and is not emitted.
//!
//! Existing quirks ported as-is (translation, don't redesign):
//! 1. R6 fallback count carries the `deleted_at IS NULL` guard twice
//!    (default manager + the explicit `filter(deleted_at__isnull=True)`)
//!    — and so does the BR4 unique check.
//! 2. Detail lookups (BR2, the scheduler-guard BR3, the derived
//!    scheduler delete lookup) carry `ORDER BY … created_at DESC` from
//!    `Meta.ordering` even though they match one row; the annotated R1/R2
//!    scheduler reads carry no default order (explicit `ORDER BY name`
//!    on R1, none on R2).
//! 3. The binding-list `pod` LEFT JOIN has no `deleted_at` filter: a
//!    soft-deleted pod row still joins and renders its stale `pod_name`.
//! 4. The R5 cascade `QuerySet.update()` bypasses `auto_now`: bindings'
//!    `updated_at` is untouched while the scheduler soft-delete refreshes
//!    `updated_at`/`updated_by_id`.
//! 5. `delete()` calls `now()` twice (`deleted_at`, then `updated_at` via
//!    `save()`): the R5b/R8 statements bind `:now` and `:now2`
//!    separately — handlers must sample `now()` twice.
//! 6. Install/patch `next_run_at` asymmetry is intentional: install
//!    writes when non-null AND different from stored (`:218`), patch
//!    writes whenever non-null (`:272`), and patch recomputes on key
//!    *presence* (`PATCH {rrule: <same>}` recomputes).
//!
//! Out of scope (sibling issues): serializer shapes + validation
//! (PIDASHCONV-629, incl. the unique-validator *decision* — only its SQL
//! lives here as BR4), route gates + feature flag (PIDASHCONV-632),
//! occurrences (PIDASHCONV-631), every response envelope and the
//! `request.user`/`crum` plumbing (handlers PIDASHCONV-633/634).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use pidash_db::tasks_ticker::models::{scheduler, scheduler_binding};

// ---------------------------------------------------------------------------
// Shared vocabulary
// ---------------------------------------------------------------------------

/// Placeholder translation rule for handlers: replace each `:name` with a
/// positional `$n` in the statement's `*_PARAMS` order (first appearance).
/// `:now`/`:now2` are separate `now()` samples (ported quirk 5); `:user_id`
/// is the `crum` request user; `:computed` is the injected-closure value.
pub const PLACEHOLDER_RULE: &str = ":name in PARAMS order -> $n";

// ---------------------------------------------------------------------------
// Scheduler reads (views.py:55-72,95-111; F36-04 R1/R2/R6)
// ---------------------------------------------------------------------------

/// R1 scheduler list (`:59-68`): the workspace row is resolved first (a
/// workspace-domain lookup, not ported here), so the scope binds the
/// already-known `:workspace_id` with no `workspaces` JOIN. Soft-deleted
/// schedulers are excluded by the default manager; the annotation counts
/// non-deleted bindings; clients observe `ORDER BY name`.
pub const SCHEDULER_LIST_SQL: &str = "SELECT \"schedulers\".\"created_at\", \"schedulers\".\"updated_at\", \"schedulers\".\"created_by_id\", \"schedulers\".\"updated_by_id\", \"schedulers\".\"deleted_at\", \"schedulers\".\"id\", \"schedulers\".\"workspace_id\", \"schedulers\".\"slug\", \"schedulers\".\"name\", \"schedulers\".\"description\", \"schedulers\".\"prompt\", \"schedulers\".\"source\", \"schedulers\".\"is_enabled\", \"schedulers\".\"color\", COUNT(\"scheduler_bindings\".\"id\") FILTER (WHERE \"scheduler_bindings\".\"deleted_at\" IS NULL) AS \"_active_binding_count\" FROM \"schedulers\" LEFT OUTER JOIN \"scheduler_bindings\" ON (\"schedulers\".\"id\" = \"scheduler_bindings\".\"scheduler_id\") WHERE (\"schedulers\".\"deleted_at\" IS NULL AND \"schedulers\".\"workspace_id\" = :workspace_id) GROUP BY \"schedulers\".\"id\" ORDER BY \"schedulers\".\"name\" ASC;";

/// Bind order for [`SCHEDULER_LIST_SQL`].
pub const SCHEDULER_LIST_PARAMS: &[&str] = &["workspace_id"];

/// R2 scheduler detail (`:98-107`, shared shape for GET/PATCH): the
/// annotation plus `pk` + `workspace__slug` scope via an `INNER JOIN
/// workspaces`. No `ORDER BY` (ported quirk 2).
pub const SCHEDULER_DETAIL_SQL: &str = "SELECT \"schedulers\".\"created_at\", \"schedulers\".\"updated_at\", \"schedulers\".\"created_by_id\", \"schedulers\".\"updated_by_id\", \"schedulers\".\"deleted_at\", \"schedulers\".\"id\", \"schedulers\".\"workspace_id\", \"schedulers\".\"slug\", \"schedulers\".\"name\", \"schedulers\".\"description\", \"schedulers\".\"prompt\", \"schedulers\".\"source\", \"schedulers\".\"is_enabled\", \"schedulers\".\"color\", COUNT(\"scheduler_bindings\".\"id\") FILTER (WHERE \"scheduler_bindings\".\"deleted_at\" IS NULL) AS \"_active_binding_count\" FROM \"schedulers\" LEFT OUTER JOIN \"scheduler_bindings\" ON (\"schedulers\".\"id\" = \"scheduler_bindings\".\"scheduler_id\") INNER JOIN \"workspaces\" ON (\"schedulers\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"schedulers\".\"deleted_at\" IS NULL AND \"schedulers\".\"id\" = :scheduler_id AND \"workspaces\".\"slug\" = :slug) GROUP BY \"schedulers\".\"id\";";

/// Bind order for [`SCHEDULER_DETAIL_SQL`].
pub const SCHEDULER_DETAIL_PARAMS: &[&str] = &["scheduler_id", "slug"];

/// Scheduler DELETE lookup (`:139-141`): same `pk` + slug scope as R2 but
/// WITHOUT the annotation (nothing is serialized afterwards), so the
/// default `Meta.ordering` applies (`ORDER BY created_at DESC`, like the
/// BR3 guard). Derived, not fixture-recorded: R2 minus the
/// annotation/`GROUP BY`, plus the default order.
pub const SCHEDULER_DELETE_LOOKUP_SQL: &str = "SELECT \"schedulers\".\"created_at\", \"schedulers\".\"updated_at\", \"schedulers\".\"created_by_id\", \"schedulers\".\"updated_by_id\", \"schedulers\".\"deleted_at\", \"schedulers\".\"id\", \"schedulers\".\"workspace_id\", \"schedulers\".\"slug\", \"schedulers\".\"name\", \"schedulers\".\"description\", \"schedulers\".\"prompt\", \"schedulers\".\"source\", \"schedulers\".\"is_enabled\", \"schedulers\".\"color\" FROM \"schedulers\" INNER JOIN \"workspaces\" ON (\"schedulers\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"schedulers\".\"deleted_at\" IS NULL AND \"schedulers\".\"id\" = :scheduler_id AND \"workspaces\".\"slug\" = :slug) ORDER BY \"schedulers\".\"created_at\" DESC;";

/// Bind order for [`SCHEDULER_DELETE_LOOKUP_SQL`].
pub const SCHEDULER_DELETE_LOOKUP_PARAMS: &[&str] = &["scheduler_id", "slug"];

/// R6 `active_binding_count` fallback
/// (`serializers/scheduler.py:108`): fires whenever the serialized row
/// was NOT annotated — i.e. on the create response (`:82-84`, always 0
/// for a new row). Ported quirk 1: the NULL guard appears twice.
pub const ACTIVE_BINDING_COUNT_SQL: &str = "SELECT COUNT(*) AS \"__count\" FROM \"scheduler_bindings\" WHERE (\"scheduler_bindings\".\"deleted_at\" IS NULL AND \"scheduler_bindings\".\"scheduler_id\" = :scheduler_id AND \"scheduler_bindings\".\"deleted_at\" IS NULL);";

/// Bind order for [`ACTIVE_BINDING_COUNT_SQL`].
pub const ACTIVE_BINDING_COUNT_PARAMS: &[&str] = &["scheduler_id"];

/// One annotated scheduler row (R1/R2): the reused
/// [`scheduler::Scheduler`] plus the `_active_binding_count` annotation
/// (`COUNT … FILTER`, never NULL).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchedulerListRow {
    /// The scheduler columns, in fixture SELECT order.
    pub scheduler: scheduler::Scheduler,
    /// `_active_binding_count`: non-deleted bindings of this scheduler.
    pub active_binding_count: i64,
}

// ---------------------------------------------------------------------------
// Scheduler writes (views.py:75-85,114-157; F36-04 R3/R4/R5)
// ---------------------------------------------------------------------------

/// R3 scheduler create (`:79-84`): provided {slug, name, prompt,
/// [description, color, is_enabled]} + pinned `:workspace_id` +
/// auto id/timestamps + `:user_id` via `crum`. `source` is the
/// `'builtin'` literal (read-only field, model default);
/// `updated_by_id`/`deleted_at` are NULL on insert. The create response
/// re-renders the un-annotated in-memory row, so handlers run R6 next.
pub const SCHEDULER_INSERT_SQL: &str = "INSERT INTO \"schedulers\" (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"workspace_id\", \"slug\", \"name\", \"description\", \"prompt\", \"source\", \"is_enabled\", \"color\") VALUES (:id, :now, :now, :user_id, NULL, NULL, :workspace_id, :slug, :name, :description, :prompt, 'builtin', :is_enabled, :color);";

/// Bind order for [`SCHEDULER_INSERT_SQL`] (`'builtin'`/NULLs are
/// literals).
pub const SCHEDULER_INSERT_PARAMS: &[&str] = &[
    "id",
    "now",
    "user_id",
    "workspace_id",
    "slug",
    "name",
    "description",
    "prompt",
    "is_enabled",
    "color",
];

/// R4 scheduler PATCH (`:127-133`): writable columns (provided values;
/// untouched ones rewritten with their current values) +
/// `updated_at`/`updated_by_id` refresh. The response re-renders the
/// in-memory row, which keeps the PRE-SAVE annotation (accurate: PATCH
/// never touches bindings) — no re-read.
pub const SCHEDULER_PATCH_SQL: &str = "UPDATE \"schedulers\" SET \"updated_at\" = :now, \"updated_by_id\" = :user_id, \"slug\" = :slug, \"name\" = :name, \"description\" = :description, \"prompt\" = :prompt, \"color\" = :color, \"is_enabled\" = :is_enabled WHERE \"schedulers\".\"id\" = :scheduler_id;";

/// Bind order for [`SCHEDULER_PATCH_SQL`].
pub const SCHEDULER_PATCH_PARAMS: &[&str] = &[
    "now",
    "user_id",
    "slug",
    "name",
    "description",
    "prompt",
    "color",
    "is_enabled",
    "scheduler_id",
];

/// R5a delete cascade, bindings first (`:151-155`): soft-delete this
/// scheduler's still-active bindings. Ported quirk 4:
/// `QuerySet.update()` bypasses `auto_now` — `updated_at` untouched.
pub const SCHEDULER_DELETE_BINDINGS_SQL: &str = "UPDATE \"scheduler_bindings\" SET \"deleted_at\" = :now WHERE (\"scheduler_bindings\".\"scheduler_id\" = :scheduler_id AND \"scheduler_bindings\".\"deleted_at\" IS NULL);";

/// Bind order for [`SCHEDULER_DELETE_BINDINGS_SQL`] (`:now` is the single
/// `timezone.now()` sampled at `:150`).
pub const SCHEDULER_DELETE_BINDINGS_PARAMS: &[&str] = &["now", "scheduler_id"];

/// R5b delete cascade, scheduler second (`:156`): `SoftDeleteModel`
/// soft-delete. Ported quirk 5: `:now`/`:now2` are two separate samples.
/// Runs in the same transaction as R5a (plus the async
/// `soft_delete_related_objects` task, which the API does not observe).
pub const SCHEDULER_SOFT_DELETE_SQL: &str = "UPDATE \"schedulers\" SET \"deleted_at\" = :now, \"updated_at\" = :now2, \"updated_by_id\" = :user_id WHERE \"schedulers\".\"id\" = :scheduler_id;";

/// Bind order for [`SCHEDULER_SOFT_DELETE_SQL`].
pub const SCHEDULER_SOFT_DELETE_PARAMS: &[&str] = &["now", "now2", "user_id", "scheduler_id"];

// ---------------------------------------------------------------------------
// Binding reads (views.py:172-186,237-249; F36-05 BR1/BR2, F36-06 columns)
// ---------------------------------------------------------------------------

/// BR1 binding list (`:175-182`): `project_id` + `workspace__slug` scope,
/// `select_related("scheduler", "last_run", "pod")` as explicit JOINs
/// (`schedulers` INNER — non-null FK; `agent_run`/`pod` LEFT — nullable
/// FKs), `ORDER BY -created_at`. Ported quirk 3: the `pod` JOIN carries
/// no `deleted_at` filter.
pub const BINDING_LIST_SQL: &str = "SELECT \"scheduler_bindings\".\"created_at\", \"scheduler_bindings\".\"updated_at\", \"scheduler_bindings\".\"created_by_id\", \"scheduler_bindings\".\"updated_by_id\", \"scheduler_bindings\".\"deleted_at\", \"scheduler_bindings\".\"id\", \"scheduler_bindings\".\"workspace_id\", \"scheduler_bindings\".\"project_id\", \"scheduler_bindings\".\"scheduler_id\", \"scheduler_bindings\".\"dtstart\", \"scheduler_bindings\".\"tzid\", \"scheduler_bindings\".\"rrule\", \"scheduler_bindings\".\"rdates\", \"scheduler_bindings\".\"exdates\", \"scheduler_bindings\".\"extra_context\", \"scheduler_bindings\".\"enabled\", \"scheduler_bindings\".\"outcome_mode\", \"scheduler_bindings\".\"next_run_at\", \"scheduler_bindings\".\"last_run_id\", \"scheduler_bindings\".\"last_error\", \"scheduler_bindings\".\"actor_id\", \"scheduler_bindings\".\"pod_id\", \"schedulers\".\"created_at\", \"schedulers\".\"updated_at\", \"schedulers\".\"created_by_id\", \"schedulers\".\"updated_by_id\", \"schedulers\".\"deleted_at\", \"schedulers\".\"id\", \"schedulers\".\"workspace_id\", \"schedulers\".\"slug\", \"schedulers\".\"name\", \"schedulers\".\"description\", \"schedulers\".\"prompt\", \"schedulers\".\"source\", \"schedulers\".\"is_enabled\", \"schedulers\".\"color\", \"agent_run\".\"id\", \"agent_run\".\"workspace_id\", \"agent_run\".\"owner_id\", \"agent_run\".\"created_by_id\", \"agent_run\".\"pod_id\", \"agent_run\".\"runner_id\", \"agent_run\".\"pinned_runner_id\", \"agent_run\".\"work_item_id\", \"agent_run\".\"scheduler_binding_id\", \"agent_run\".\"parent_run_id\", \"agent_run\".\"status\", \"agent_run\".\"executor_kind\", \"agent_run\".\"dispatch_attempts\", \"agent_run\".\"cancel_requested_at\", \"agent_run\".\"cancel_reason\", \"agent_run\".\"error_code\", \"agent_run\".\"tool_plan\", \"agent_run\".\"terminal_hooks_applied_at\", \"agent_run\".\"terminal_capacity_released_at\", \"agent_run\".\"prompt\", \"agent_run\".\"trigger\", \"agent_run\".\"prompt_manifest\", \"agent_run\".\"phase_kind\", \"agent_run\".\"run_config\", \"agent_run\".\"required_capabilities\", \"agent_run\".\"thread_id\", \"agent_run\".\"agent_metadata\", \"agent_run\".\"lease_expires_at\", \"agent_run\".\"done_payload\", \"agent_run\".\"error\", \"agent_run\".\"refusal_category\", \"agent_run\".\"llm_model\", \"agent_run\".\"usage\", \"agent_run\".\"input_tokens\", \"agent_run\".\"output_tokens\", \"agent_run\".\"total_tokens\", \"agent_run\".\"created_at\", \"agent_run\".\"assigned_at\", \"agent_run\".\"queue_position\", \"agent_run\".\"started_at\", \"agent_run\".\"ended_at\", \"pod\".\"id\", \"pod\".\"workspace_id\", \"pod\".\"project_id\", \"pod\".\"name\", \"pod\".\"description\", \"pod\".\"created_by_id\", \"pod\".\"is_default\", \"pod\".\"deleted_at\", \"pod\".\"created_at\", \"pod\".\"updated_at\" FROM \"scheduler_bindings\" INNER JOIN \"workspaces\" ON (\"scheduler_bindings\".\"workspace_id\" = \"workspaces\".\"id\") INNER JOIN \"schedulers\" ON (\"scheduler_bindings\".\"scheduler_id\" = \"schedulers\".\"id\") LEFT OUTER JOIN \"agent_run\" ON (\"scheduler_bindings\".\"last_run_id\" = \"agent_run\".\"id\") LEFT OUTER JOIN \"pod\" ON (\"scheduler_bindings\".\"pod_id\" = \"pod\".\"id\") WHERE (\"scheduler_bindings\".\"deleted_at\" IS NULL AND \"scheduler_bindings\".\"project_id\" = :project_id AND \"workspaces\".\"slug\" = :slug) ORDER BY \"scheduler_bindings\".\"created_at\" DESC;";

/// Bind order for [`BINDING_LIST_SQL`].
pub const BINDING_LIST_PARAMS: &[&str] = &["project_id", "slug"];

/// BR2 binding detail lookup (`:240-245`; same shape for the PATCH
/// lookup `:255-260` and the uninstall lookup `:284-289`): `pk` +
/// `project_id` + slug scope with NO `select_related` (the `workspaces`
/// JOIN only) — the serializer's scheduler/last_run/pod dereferences
/// each fire one extra query (N+1 vs BR1; port the queries, not the
/// count).
pub const BINDING_DETAIL_SQL: &str = "SELECT \"scheduler_bindings\".\"created_at\", \"scheduler_bindings\".\"updated_at\", \"scheduler_bindings\".\"created_by_id\", \"scheduler_bindings\".\"updated_by_id\", \"scheduler_bindings\".\"deleted_at\", \"scheduler_bindings\".\"id\", \"scheduler_bindings\".\"workspace_id\", \"scheduler_bindings\".\"project_id\", \"scheduler_bindings\".\"scheduler_id\", \"scheduler_bindings\".\"dtstart\", \"scheduler_bindings\".\"tzid\", \"scheduler_bindings\".\"rrule\", \"scheduler_bindings\".\"rdates\", \"scheduler_bindings\".\"exdates\", \"scheduler_bindings\".\"extra_context\", \"scheduler_bindings\".\"enabled\", \"scheduler_bindings\".\"outcome_mode\", \"scheduler_bindings\".\"next_run_at\", \"scheduler_bindings\".\"last_run_id\", \"scheduler_bindings\".\"last_error\", \"scheduler_bindings\".\"actor_id\", \"scheduler_bindings\".\"pod_id\" FROM \"scheduler_bindings\" INNER JOIN \"workspaces\" ON (\"scheduler_bindings\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"scheduler_bindings\".\"deleted_at\" IS NULL AND \"scheduler_bindings\".\"id\" = :binding_id AND \"scheduler_bindings\".\"project_id\" = :project_id AND \"workspaces\".\"slug\" = :slug) ORDER BY \"scheduler_bindings\".\"created_at\" DESC;";

/// Bind order for [`BINDING_DETAIL_SQL`].
pub const BINDING_DETAIL_PARAMS: &[&str] = &["binding_id", "project_id", "slug"];

/// `pod` table name (`runner/models.py`, `Meta.db_table`).
pub const POD_TABLE: &str = "pod";

/// `pod` columns in Django `_meta` field order (F36-06
/// `pod_lastrun_columns.json`, `pod.columns`). D-36 reads `name` (→
/// `pod_name`), `project_id` (validation), `id` (pod PK field).
pub const POD_COLUMNS: &[&str] = &[
    "id",
    "workspace_id",
    "project_id",
    "name",
    "description",
    "created_by_id",
    "is_default",
    "deleted_at",
    "created_at",
    "updated_at",
];

/// `agent_run` table name (`runner/models.py`, `Meta.db_table`). Note:
/// `agent_run` has NO `deleted_at` column (not a `SoftDeleteModel`).
pub const AGENT_RUN_TABLE: &str = "agent_run";

/// `agent_run` columns in Django `_meta` field order (F36-06
/// `pod_lastrun_columns.json`, `agent_run.columns`). D-36 reads `status`
/// (→ `last_run_status`) and `ended_at` (→ `last_run_ended_at`) on BR1.
/// `input/output/total_tokens` are Postgres-generated `bigint`s from the
/// `usage` JSON bag (nullable until written).
pub const AGENT_RUN_COLUMNS: &[&str] = &[
    "id",
    "workspace_id",
    "owner_id",
    "created_by_id",
    "pod_id",
    "runner_id",
    "pinned_runner_id",
    "work_item_id",
    "scheduler_binding_id",
    "parent_run_id",
    "status",
    "executor_kind",
    "dispatch_attempts",
    "cancel_requested_at",
    "cancel_reason",
    "error_code",
    "tool_plan",
    "terminal_hooks_applied_at",
    "terminal_capacity_released_at",
    "prompt",
    "trigger",
    "prompt_manifest",
    "phase_kind",
    "run_config",
    "required_capabilities",
    "thread_id",
    "agent_metadata",
    "lease_expires_at",
    "done_payload",
    "error",
    "refusal_category",
    "llm_model",
    "usage",
    "input_tokens",
    "output_tokens",
    "total_tokens",
    "created_at",
    "assigned_at",
    "queue_position",
    "started_at",
    "ended_at",
];

/// One `pod` row as selected by BR1 (F36-06 columns; table-level read —
/// the Pod model port belongs to D-15 L2, not this module). Field names
/// match the columns exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PodRow {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub project_id: uuid::Uuid,
    pub name: String,
    pub description: String,
    pub created_by_id: Option<uuid::Uuid>,
    pub is_default: bool,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One `agent_run` row as selected by BR1 (F36-06 columns; table-level
/// read — the AgentRun model port belongs to dispatch, not this
/// module). Field names match the columns exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRunRow {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub owner_id: Option<uuid::Uuid>,
    pub created_by_id: uuid::Uuid,
    pub pod_id: uuid::Uuid,
    pub runner_id: Option<uuid::Uuid>,
    pub pinned_runner_id: Option<uuid::Uuid>,
    pub work_item_id: Option<uuid::Uuid>,
    pub scheduler_binding_id: Option<uuid::Uuid>,
    pub parent_run_id: Option<uuid::Uuid>,
    pub status: String,
    pub executor_kind: String,
    pub dispatch_attempts: i32,
    pub cancel_requested_at: Option<DateTime<Utc>>,
    pub cancel_reason: String,
    pub error_code: String,
    pub tool_plan: serde_json::Value,
    pub terminal_hooks_applied_at: Option<DateTime<Utc>>,
    pub terminal_capacity_released_at: Option<DateTime<Utc>>,
    pub prompt: String,
    pub trigger: String,
    pub prompt_manifest: Option<serde_json::Value>,
    pub phase_kind: String,
    pub run_config: serde_json::Value,
    pub required_capabilities: serde_json::Value,
    pub thread_id: String,
    pub agent_metadata: serde_json::Value,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub done_payload: Option<serde_json::Value>,
    pub error: String,
    pub refusal_category: String,
    pub llm_model: String,
    pub usage: serde_json::Value,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub assigned_at: Option<DateTime<Utc>>,
    pub queue_position: Option<i16>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
}

/// One joined binding-list row (BR1): the reused
/// [`scheduler_binding::SchedulerBinding`] plus its INNER-joined
/// [`scheduler::Scheduler`] and the LEFT-joined last run / pod (each
/// `None` exactly when its FK is NULL — every joined column is NULL
/// then).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BindingListRow {
    /// The binding columns, in fixture SELECT order.
    pub binding: scheduler_binding::SchedulerBinding,
    /// INNER-joined scheduler (non-null FK — always present).
    pub scheduler: scheduler::Scheduler,
    /// LEFT-joined last run (`None` iff `binding.last_run_id` is NULL).
    pub last_run: Option<AgentRunRow>,
    /// LEFT-joined pod (`None` iff `binding.pod_id` is NULL).
    pub pod: Option<PodRow>,
}

// ---------------------------------------------------------------------------
// Binding writes (views.py:189-224,252-291; F36-05 BR3-BR8)
// ---------------------------------------------------------------------------

/// BR3 install project lookup (`:192`): `pk` + slug scope; a miss 404s
/// (`No Project …`). Only `id`/`workspace_id` are read afterwards, so
/// only those are selected.
pub const INSTALL_PROJECT_LOOKUP_SQL: &str = "SELECT \"projects\".\"id\", \"projects\".\"workspace_id\" FROM \"projects\" INNER JOIN \"workspaces\" ON (\"projects\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"projects\".\"deleted_at\" IS NULL AND \"projects\".\"id\" = :project_id AND \"workspaces\".\"slug\" = :slug) ORDER BY \"projects\".\"created_at\" DESC;";

/// Bind order for [`INSTALL_PROJECT_LOOKUP_SQL`].
pub const INSTALL_PROJECT_LOOKUP_PARAMS: &[&str] = &["project_id", "slug"];

/// BR3 install scheduler guard (`:194-199`): `pk` + SAME workspace +
/// `is_enabled` — a miss (disabled, foreign-workspace, soft-deleted, or
/// absent-from-body id) 404s (`No Scheduler …`). Runs BEFORE serializer
/// validation, so a bad id 404s instead of 400ing. (A malformed
/// non-UUID id instead 400s via `handle_exception` — guards layer.)
pub const INSTALL_SCHEDULER_GUARD_SQL: &str = "SELECT \"schedulers\".\"created_at\", \"schedulers\".\"updated_at\", \"schedulers\".\"created_by_id\", \"schedulers\".\"updated_by_id\", \"schedulers\".\"deleted_at\", \"schedulers\".\"id\", \"schedulers\".\"workspace_id\", \"schedulers\".\"slug\", \"schedulers\".\"name\", \"schedulers\".\"description\", \"schedulers\".\"prompt\", \"schedulers\".\"source\", \"schedulers\".\"is_enabled\", \"schedulers\".\"color\" FROM \"schedulers\" WHERE (\"schedulers\".\"deleted_at\" IS NULL AND \"schedulers\".\"is_enabled\" AND \"schedulers\".\"id\" = :scheduler_id AND \"schedulers\".\"workspace_id\" = :workspace_id) ORDER BY \"schedulers\".\"created_at\" DESC;";

/// Bind order for [`INSTALL_SCHEDULER_GUARD_SQL`] (`:workspace_id` is the
/// looked-up project's workspace).
pub const INSTALL_SCHEDULER_GUARD_PARAMS: &[&str] = &["scheduler_id", "workspace_id"];

/// BR4 unique check (DRF `UniqueTogetherValidator`, validator `.exists()`
/// over the double-deleted_at-NULL-guarded queryset for
/// (scheduler, project); ported quirk 1 again). Only the SQL lives here;
/// the validator *decision* is the serializers layer's.
pub const BINDING_UNIQUE_CHECK_SQL: &str = "SELECT 1 AS \"a\" FROM \"scheduler_bindings\" WHERE (\"scheduler_bindings\".\"deleted_at\" IS NULL AND \"scheduler_bindings\".\"deleted_at\" IS NULL AND \"scheduler_bindings\".\"project_id\" = :project_id AND \"scheduler_bindings\".\"scheduler_id\" = :scheduler_id) LIMIT 1;";

/// Bind order for [`BINDING_UNIQUE_CHECK_SQL`].
pub const BINDING_UNIQUE_CHECK_PARAMS: &[&str] = &["project_id", "scheduler_id"];

/// BR4 self-row exclusion on PATCH: spliced inside the WHERE parens
/// (before the closing paren) so the row being patched never matches
/// itself. See [`binding_unique_check_sql`].
pub const BINDING_UNIQUE_CHECK_PATCH_EXCLUSION: &str =
    "AND NOT (\"scheduler_bindings\".\"id\" = :binding_id)";

/// Bind order for the PATCH form of [`BINDING_UNIQUE_CHECK_SQL`].
pub const BINDING_UNIQUE_CHECK_PATCH_PARAMS: &[&str] =
    &["project_id", "scheduler_id", "binding_id"];

/// BR4 with or without the PATCH self-row exclusion. `false` returns
/// [`BINDING_UNIQUE_CHECK_SQL`] byte-for-byte (install path).
pub fn binding_unique_check_sql(exclude_binding: bool) -> String {
    if !exclude_binding {
        return BINDING_UNIQUE_CHECK_SQL.to_owned();
    }
    BINDING_UNIQUE_CHECK_SQL.replacen(
        ") LIMIT 1;",
        &format!(" {BINDING_UNIQUE_CHECK_PATCH_EXCLUSION}) LIMIT 1;"),
        1,
    )
}

/// BR5 install INSERT (`:207-212`): validated fields + pinned
/// scheduler/project/workspace/actor (`actor` is `request.user` when
/// authenticated else NULL; `created_by_id` is the `crum` user).
/// Omitted keys take model defaults (`tzid 'UTC'`, `rrule ''`,
/// `rdates`/`exdates` `[]`, `extra_context ''`, `enabled true`,
/// `outcome_mode 'create_issue'`, `next_run_at`/`last_run` NULL,
/// `last_error ''`, `pod` NULL). `WorkspaceBaseModel.save()` re-pins
/// `workspace` from `project.workspace` (same value, no extra write).
pub const BINDING_INSERT_SQL: &str = "INSERT INTO \"scheduler_bindings\" (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"workspace_id\", \"project_id\", \"scheduler_id\", \"dtstart\", \"tzid\", \"rrule\", \"rdates\", \"exdates\", \"extra_context\", \"enabled\", \"outcome_mode\", \"next_run_at\", \"last_run_id\", \"last_error\", \"actor_id\", \"pod_id\") VALUES (:id, :now, :now, :user_id, NULL, NULL, :workspace_id, :project_id, :scheduler_id, :dtstart, :tzid, :rrule, :rdates, :exdates, :extra_context, :enabled, :outcome_mode, NULL, NULL, '', :actor_id, :pod_id);";

/// Bind order for [`BINDING_INSERT_SQL`] (NULLs/`''` are literals).
pub const BINDING_INSERT_PARAMS: &[&str] = &[
    "id",
    "now",
    "user_id",
    "workspace_id",
    "project_id",
    "scheduler_id",
    "dtstart",
    "tzid",
    "rrule",
    "rdates",
    "exdates",
    "extra_context",
    "enabled",
    "outcome_mode",
    "actor_id",
    "pod_id",
];

/// BR6 `next_run_at` write-back (`:218-220` install / `:272-274` patch):
/// `save(update_fields=["next_run_at", "updated_at"])` — only these two
/// columns (`updated_by` is NOT rewritten, unlike R4/R7).
pub const NEXT_RUN_AT_WRITEBACK_SQL: &str = "UPDATE \"scheduler_bindings\" SET \"next_run_at\" = :computed, \"updated_at\" = :now WHERE \"scheduler_bindings\".\"id\" = :binding_id;";

/// Bind order for [`NEXT_RUN_AT_WRITEBACK_SQL`] (`:computed` is the
/// injected-closure value that passed the path's write decision).
pub const NEXT_RUN_AT_WRITEBACK_PARAMS: &[&str] = &["computed", "now", "binding_id"];

/// BR7 PATCH save (`:263`): writable columns (provided values;
/// untouched ones rewritten) + `updated_at`/`updated_by_id` refresh,
/// then the conditional BR6. (`scheduler`/`project` are locked by
/// validation — never in the SET list.)
pub const BINDING_PATCH_SQL: &str = "UPDATE \"scheduler_bindings\" SET \"updated_at\" = :now, \"updated_by_id\" = :user_id, \"dtstart\" = :dtstart, \"tzid\" = :tzid, \"rrule\" = :rrule, \"rdates\" = :rdates, \"exdates\" = :exdates, \"extra_context\" = :extra_context, \"enabled\" = :enabled, \"outcome_mode\" = :outcome_mode, \"pod_id\" = :pod_id WHERE \"scheduler_bindings\".\"id\" = :binding_id;";

/// Bind order for [`BINDING_PATCH_SQL`].
pub const BINDING_PATCH_PARAMS: &[&str] = &[
    "now",
    "user_id",
    "dtstart",
    "tzid",
    "rrule",
    "rdates",
    "exdates",
    "extra_context",
    "enabled",
    "outcome_mode",
    "pod_id",
    "binding_id",
];

/// BR8 uninstall (`:290`): `binding.delete()` soft-delete (204, empty
/// body). Ported quirk 5 again: `:now`/`:now2` are separate samples.
/// (Also enqueues the async cascade task, unobserved by the API.)
pub const BINDING_UNINSTALL_SQL: &str = "UPDATE \"scheduler_bindings\" SET \"deleted_at\" = :now, \"updated_at\" = :now2, \"updated_by_id\" = :user_id WHERE \"scheduler_bindings\".\"id\" = :binding_id;";

/// Bind order for [`BINDING_UNINSTALL_SQL`].
pub const BINDING_UNINSTALL_PARAMS: &[&str] = &["now", "now2", "user_id", "binding_id"];

// ---------------------------------------------------------------------------
// next_run_at recompute decisions (views.py:216-220,266-274; F36-05 BR6)
// ---------------------------------------------------------------------------

/// RRULE-bundle keys whose *presence* in the PATCH body triggers a
/// recompute (`:266-268`), in source order.
pub const RECOMPUTE_TRIGGER_KEYS: &[&str] = &["dtstart", "rrule", "rdates", "exdates", "tzid"];

/// The RRULE bundle a recompute expands: exactly the
/// `_next_fire_for_binding` inputs (`bgtasks/scheduler.py:70-83`).
/// On install the handler fills it from the just-saved row (stored
/// values incl. defaults for omitted keys); on patch from the row AFTER
/// `refresh_from_db` (`:270`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RruleBundle<'a> {
    /// Series anchor (non-null column).
    pub dtstart: DateTime<Utc>,
    /// Stored `rrule` (`""` = single-shot at `dtstart`).
    pub rrule: &'a str,
    /// Stored `tzid` (informational today; expansion runs in UTC).
    pub tzid: &'a str,
    /// Raw stored `rdates` JSON (the closure coerces it).
    pub rdates: &'a serde_json::Value,
    /// Raw stored `exdates` JSON (the closure coerces it).
    pub exdates: &'a serde_json::Value,
}

/// Patch recompute trigger (`:266-268`): true when ANY bundle key is
/// present in the request body — presence, not value change, and an
/// explicit-null key still triggers (Python `in` on `request.data`).
/// `PATCH {enabled: false}` never recomputes; `PATCH {rrule: <same>}`
/// does.
pub fn patch_triggers_recompute(body: &serde_json::Value) -> bool {
    RECOMPUTE_TRIGGER_KEYS
        .iter()
        .any(|key| body.get(key).is_some())
}

/// Install write decision (`:218`): write the computed value back via
/// BR6 only when it is non-null AND differs from the stored value.
/// (Stored is NULL at that point, so in practice "when non-null" —
/// the comparison is still ported, not simplified away.)
pub fn install_next_run_at_decision(
    computed: Option<DateTime<Utc>>,
    stored: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    computed.filter(|next| Some(*next) != stored)
}

/// Patch write decision (`:272`): write the computed value back via BR6
/// whenever it is non-null — WITHOUT comparing to stored (ported quirk
/// 6: the install/patch asymmetry is intentional).
pub fn patch_next_run_at_decision(computed: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    computed
}

/// Install recompute (`:216-220`): expand `bundle` through the injected
/// `next_fire_for_binding` closure, then apply
/// [`install_next_run_at_decision`] against the just-saved row's stored
/// value. Returns the value to write via BR6, or `None` for no write.
///
/// The closure ports `_next_fire_for_binding`'s exact call shape
/// (`bgtasks/scheduler.py:70-83`): the bindings handlers pass one that
/// applies the `rrule or ""` / `tzid or "UTC"` guards (the former is a
/// no-op on `&str`; the latter is unobservable since expansion runs in
/// UTC), coerces both JSON lists with the jobs crate's
/// `coerce_iso_datetimes`, and expands with `next_fire_from_rrule`.
/// This module never depends on the jobs crate (the graph is acyclic).
pub fn install_next_run_at(
    bundle: &RruleBundle<'_>,
    stored: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    next_fire_for_binding: impl Fn(&RruleBundle<'_>, DateTime<Utc>) -> Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    let computed = next_fire_for_binding(bundle, now);
    install_next_run_at_decision(computed, stored)
}

/// Patch recompute (`:269-274`): expand `bundle` through the injected
/// closure, then apply [`patch_next_run_at_decision`]. The handler
/// calls this only after [`patch_triggers_recompute`] passed AND the
/// row was refreshed from the DB (`:270`), and writes the result via
/// BR6 (also updating its in-memory row, which the 200 re-renders).
pub fn patch_next_run_at(
    bundle: &RruleBundle<'_>,
    now: DateTime<Utc>,
    next_fire_for_binding: impl Fn(&RruleBundle<'_>, DateTime<Utc>) -> Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    let computed = next_fire_for_binding(bundle, now);
    patch_next_run_at_decision(computed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_SCHEDULER_SQL: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_scheduler/queries/scheduler_sql.sql"
    );
    const FIXTURE_SCHEDULER_ROWS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_scheduler/queries/scheduler_sql.rows.json"
    );
    const FIXTURE_BINDING_SQL: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_scheduler/queries/binding_sql.sql"
    );
    const FIXTURE_BINDING_ROWS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_scheduler/queries/binding_sql.rows.json"
    );
    const FIXTURE_POD_LASTRUN: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_scheduler/queries/pod_lastrun_columns.json"
    );

    fn fixture_sql(path: &str) -> String {
        std::fs::read_to_string(path).expect("fixture SQL exists")
    }

    fn fixture_json(path: &str) -> serde_json::Value {
        let raw = std::fs::read_to_string(path).expect("fixture JSON exists");
        serde_json::from_str(&raw).expect("fixture is valid JSON")
    }

    /// Statements under the `-- R…` marker starting with `prefix`:
    /// following non-comment non-empty lines until the next marker.
    fn section_statements(sql: &str, prefix: &str) -> Vec<String> {
        let mut found = false;
        let mut out = Vec::new();
        for line in sql.lines() {
            if line.starts_with("-- R") {
                if found {
                    break;
                }
                if line.starts_with(prefix) {
                    // Skip the prose markers (they yield no statements;
                    // the SQL marker with the same R-number follows).
                    found = true;
                }
                continue;
            }
            if found {
                if line.trim().is_empty() || line.starts_with("--") {
                    continue;
                }
                out.push(line.to_owned());
            }
        }
        // A prose marker matched first yields nothing; retry from the
        // SQL marker when the first hit was prose.
        if out.is_empty() {
            let mut skipped_first = false;
            let mut retry = Vec::new();
            let mut active = false;
            for line in sql.lines() {
                if line.starts_with("-- R") {
                    if active {
                        break;
                    }
                    if line.starts_with(prefix) {
                        if skipped_first {
                            active = true;
                        } else {
                            skipped_first = true;
                        }
                    }
                    continue;
                }
                if active {
                    if line.trim().is_empty() || line.starts_with("--") {
                        continue;
                    }
                    retry.push(line.to_owned());
                }
            }
            return retry;
        }
        out
    }

    fn section_statement(sql: &str, prefix: &str) -> String {
        let stmts = section_statements(sql, prefix);
        assert_eq!(stmts.len(), 1, "one statement under {prefix}");
        stmts.into_iter().next().expect("one statement")
    }

    /// Distinct `:name` placeholders in first-appearance order.
    fn placeholders_in(sql: &str) -> Vec<String> {
        let mut seen = Vec::new();
        let bytes = sql.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b':' {
                let mut j = i + 1;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                if j > i + 1 {
                    let name = sql[i + 1..j].to_owned();
                    if !seen.contains(&name) {
                        seen.push(name);
                    }
                    i = j;
                    continue;
                }
            }
            i += 1;
        }
        seen
    }

    fn quoted_select_list(table: &str, columns: &[&str]) -> String {
        columns
            .iter()
            .map(|c| format!("\"{table}\".\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ")
    }

    #[test]
    fn scheduler_statements_byte_match_f36_04() {
        let sql = fixture_sql(FIXTURE_SCHEDULER_SQL);
        assert_eq!(
            section_statement(&sql, "-- R1 scheduler list"),
            SCHEDULER_LIST_SQL
        );
        assert_eq!(
            section_statement(&sql, "-- R2 scheduler detail"),
            SCHEDULER_DETAIL_SQL
        );
        assert_eq!(
            section_statement(&sql, "-- R3 scheduler create"),
            SCHEDULER_INSERT_SQL
        );
        assert_eq!(
            section_statement(&sql, "-- R4 scheduler PATCH"),
            SCHEDULER_PATCH_SQL
        );
        assert_eq!(
            section_statement(&sql, "-- R6 active_binding_count"),
            ACTIVE_BINDING_COUNT_SQL
        );
        // R5: one transaction { bindings UPDATE, scheduler UPDATE }.
        assert_eq!(
            section_statements(&sql, "-- R5 scheduler DELETE cascade"),
            vec![
                "BEGIN;".to_owned(),
                SCHEDULER_DELETE_BINDINGS_SQL.to_owned(),
                SCHEDULER_SOFT_DELETE_SQL.to_owned(),
                "COMMIT;".to_owned(),
            ]
        );
    }

    #[test]
    fn binding_statements_byte_match_f36_05() {
        let sql = fixture_sql(FIXTURE_BINDING_SQL);
        assert_eq!(
            section_statement(&sql, "-- R1 binding list"),
            BINDING_LIST_SQL
        );
        assert_eq!(
            section_statement(&sql, "-- R2 binding detail lookup"),
            BINDING_DETAIL_SQL
        );
        assert_eq!(
            section_statement(&sql, "-- R3 install project lookup"),
            INSTALL_PROJECT_LOOKUP_SQL
        );
        assert_eq!(
            section_statement(&sql, "-- R3 install scheduler guard"),
            INSTALL_SCHEDULER_GUARD_SQL
        );
        assert_eq!(
            section_statement(&sql, "-- R4 unique check"),
            BINDING_UNIQUE_CHECK_SQL
        );
        assert_eq!(
            section_statement(&sql, "-- R5 install INSERT"),
            BINDING_INSERT_SQL
        );
        assert_eq!(
            section_statement(&sql, "-- R6 next_run_at write-back"),
            NEXT_RUN_AT_WRITEBACK_SQL
        );
        assert_eq!(
            section_statement(&sql, "-- R7 PATCH save"),
            BINDING_PATCH_SQL
        );
        assert_eq!(
            section_statement(&sql, "-- R8 uninstall"),
            BINDING_UNINSTALL_SQL
        );
    }

    #[test]
    fn scheduler_select_lists_follow_model_column_order() {
        // The SELECT prefixes are the db-layer COLUMNS in order — quoted
        // and table-qualified per table.
        let schedulers = quoted_select_list(scheduler::TABLE, scheduler::COLUMNS);
        assert!(
            SCHEDULER_LIST_SQL.starts_with(&format!("SELECT {schedulers}, COUNT(")),
            "R1 selects all scheduler columns first"
        );
        assert!(
            SCHEDULER_DETAIL_SQL.starts_with(&format!("SELECT {schedulers}, COUNT(")),
            "R2 selects all scheduler columns first"
        );
        assert!(
            SCHEDULER_DELETE_LOOKUP_SQL.starts_with(&format!("SELECT {schedulers} FROM ")),
            "delete lookup selects all scheduler columns, no annotation"
        );
        let bindings = quoted_select_list(scheduler_binding::TABLE, scheduler_binding::COLUMNS);
        assert!(
            BINDING_LIST_SQL.starts_with(&format!("SELECT {bindings}, ")),
            "BR1 selects all binding columns first"
        );
        assert!(
            BINDING_DETAIL_SQL.starts_with(&format!("SELECT {bindings} FROM ")),
            "BR2 selects all binding columns, no joins beyond workspaces"
        );
    }

    #[test]
    fn binding_list_joins_follow_f36_06_column_order() {
        // BR1 select_related shape: bindings, schedulers, agent_run, pod —
        // each in its pinned column order.
        let expected = format!(
            "SELECT {}, {}, {}, {} FROM ",
            quoted_select_list(scheduler_binding::TABLE, scheduler_binding::COLUMNS),
            quoted_select_list(scheduler::TABLE, scheduler::COLUMNS),
            quoted_select_list(AGENT_RUN_TABLE, AGENT_RUN_COLUMNS),
            quoted_select_list(POD_TABLE, POD_COLUMNS),
        );
        assert!(
            BINDING_LIST_SQL.starts_with(&expected),
            "BR1 SELECT list is bindings+schedulers+agent_run+pod in order"
        );
        for needle in [
            "INNER JOIN \"workspaces\" ON (\"scheduler_bindings\".\"workspace_id\" = \"workspaces\".\"id\")",
            "INNER JOIN \"schedulers\" ON (\"scheduler_bindings\".\"scheduler_id\" = \"schedulers\".\"id\")",
            "LEFT OUTER JOIN \"agent_run\" ON (\"scheduler_bindings\".\"last_run_id\" = \"agent_run\".\"id\")",
            "LEFT OUTER JOIN \"pod\" ON (\"scheduler_bindings\".\"pod_id\" = \"pod\".\"id\")",
        ] {
            assert!(BINDING_LIST_SQL.contains(needle), "missing {needle}");
        }
        // Ported quirk 3: no tombstone filter on the pod join.
        let pod_join = BINDING_LIST_SQL
            .find("LEFT OUTER JOIN \"pod\"")
            .expect("pod join");
        let where_at = BINDING_LIST_SQL.find(" WHERE (").expect("where");
        assert!(!BINDING_LIST_SQL[pod_join..where_at].contains("deleted_at"));
    }

    #[test]
    fn scoping_order_and_guards_match_python() {
        // Tenant scoping on every read.
        assert!(SCHEDULER_LIST_SQL.contains("\"schedulers\".\"workspace_id\" = :workspace_id"));
        assert!(SCHEDULER_DETAIL_SQL.contains("\"workspaces\".\"slug\" = :slug"));
        assert!(BINDING_LIST_SQL.contains("\"scheduler_bindings\".\"project_id\" = :project_id"));
        assert!(BINDING_DETAIL_SQL.contains("\"workspaces\".\"slug\" = :slug"));
        // Soft-delete scoping on every read.
        for sql in [
            SCHEDULER_LIST_SQL,
            SCHEDULER_DETAIL_SQL,
            SCHEDULER_DELETE_LOOKUP_SQL,
            ACTIVE_BINDING_COUNT_SQL,
            BINDING_LIST_SQL,
            BINDING_DETAIL_SQL,
            INSTALL_PROJECT_LOOKUP_SQL,
            INSTALL_SCHEDULER_GUARD_SQL,
            BINDING_UNIQUE_CHECK_SQL,
        ] {
            assert!(
                sql.contains("deleted_at\" IS NULL"),
                "missing tombstone guard"
            );
        }
        // Orders: list by name ASC; detail/default paths by -created_at;
        // R2 carries no ORDER BY at all (ported quirk 2).
        assert!(SCHEDULER_LIST_SQL.ends_with("ORDER BY \"schedulers\".\"name\" ASC;"));
        assert!(!SCHEDULER_DETAIL_SQL.contains("ORDER BY"));
        assert!(
            SCHEDULER_DELETE_LOOKUP_SQL.ends_with("ORDER BY \"schedulers\".\"created_at\" DESC;")
        );
        assert!(BINDING_LIST_SQL.ends_with("ORDER BY \"scheduler_bindings\".\"created_at\" DESC;"));
        assert!(
            BINDING_DETAIL_SQL.ends_with("ORDER BY \"scheduler_bindings\".\"created_at\" DESC;")
        );
        // The annotation counts non-deleted bindings only.
        assert!(SCHEDULER_LIST_SQL.contains(
            "COUNT(\"scheduler_bindings\".\"id\") FILTER (WHERE \"scheduler_bindings\".\"deleted_at\" IS NULL) AS \"_active_binding_count\""
        ));
        assert!(SCHEDULER_LIST_SQL.contains("GROUP BY \"schedulers\".\"id\""));
        // Scheduler guard requires an enabled scheduler in the same
        // workspace (BR3).
        assert!(INSTALL_SCHEDULER_GUARD_SQL.contains("\"schedulers\".\"is_enabled\""));
        assert!(
            INSTALL_SCHEDULER_GUARD_SQL.contains("\"schedulers\".\"workspace_id\" = :workspace_id")
        );
        // Ported quirk 1: the NULL guard appears twice in R6 and BR4.
        assert_eq!(
            ACTIVE_BINDING_COUNT_SQL
                .matches("deleted_at\" IS NULL")
                .count(),
            2
        );
        assert_eq!(
            BINDING_UNIQUE_CHECK_SQL
                .matches("deleted_at\" IS NULL")
                .count(),
            2
        );
        // Ported quirk 4: the cascade UPDATE touches deleted_at only.
        assert_eq!(
            SCHEDULER_DELETE_BINDINGS_SQL
                .matches("SET \"deleted_at\" = :now")
                .count(),
            1
        );
        assert!(!SCHEDULER_DELETE_BINDINGS_SQL.contains("updated_at"));
    }

    #[test]
    fn params_cover_every_placeholder_in_order() {
        for (sql, params) in [
            (SCHEDULER_LIST_SQL, SCHEDULER_LIST_PARAMS),
            (SCHEDULER_DETAIL_SQL, SCHEDULER_DETAIL_PARAMS),
            (SCHEDULER_DELETE_LOOKUP_SQL, SCHEDULER_DELETE_LOOKUP_PARAMS),
            (SCHEDULER_INSERT_SQL, SCHEDULER_INSERT_PARAMS),
            (SCHEDULER_PATCH_SQL, SCHEDULER_PATCH_PARAMS),
            (
                SCHEDULER_DELETE_BINDINGS_SQL,
                SCHEDULER_DELETE_BINDINGS_PARAMS,
            ),
            (SCHEDULER_SOFT_DELETE_SQL, SCHEDULER_SOFT_DELETE_PARAMS),
            (ACTIVE_BINDING_COUNT_SQL, ACTIVE_BINDING_COUNT_PARAMS),
            (BINDING_LIST_SQL, BINDING_LIST_PARAMS),
            (BINDING_DETAIL_SQL, BINDING_DETAIL_PARAMS),
            (INSTALL_PROJECT_LOOKUP_SQL, INSTALL_PROJECT_LOOKUP_PARAMS),
            (INSTALL_SCHEDULER_GUARD_SQL, INSTALL_SCHEDULER_GUARD_PARAMS),
            (BINDING_UNIQUE_CHECK_SQL, BINDING_UNIQUE_CHECK_PARAMS),
            (BINDING_INSERT_SQL, BINDING_INSERT_PARAMS),
            (NEXT_RUN_AT_WRITEBACK_SQL, NEXT_RUN_AT_WRITEBACK_PARAMS),
            (BINDING_PATCH_SQL, BINDING_PATCH_PARAMS),
            (BINDING_UNINSTALL_SQL, BINDING_UNINSTALL_PARAMS),
        ] {
            let found = placeholders_in(sql);
            let expected: Vec<String> = params.iter().map(|p| (*p).to_owned()).collect();
            assert_eq!(found, expected, "PARAMS mismatch in {sql:.60}…");
        }
        // The PATCH unique check adds exactly the self-row exclusion.
        let patch = binding_unique_check_sql(true);
        assert!(patch.contains(BINDING_UNIQUE_CHECK_PATCH_EXCLUSION));
        assert_eq!(
            placeholders_in(&patch),
            BINDING_UNIQUE_CHECK_PATCH_PARAMS
                .iter()
                .map(|p| (*p).to_owned())
                .collect::<Vec<_>>()
        );
        assert_eq!(binding_unique_check_sql(false), BINDING_UNIQUE_CHECK_SQL);
    }

    #[test]
    fn scheduler_rows_match_f36_04() {
        let fixture = fixture_json(FIXTURE_SCHEDULER_ROWS);
        assert_eq!(fixture["source"], "app/views/scheduler/views.py:46-157");
        let rows = fixture["rows"].as_array().expect("rows array");
        assert_eq!(rows.len(), 3);
        // List order is by name ASC; counts render, never null.
        let names: Vec<&str> = rows
            .iter()
            .map(|r| r["name"].as_str().expect("name"))
            .collect();
        assert_eq!(names, vec!["A First", "B Second", "M Middle"]);
        assert_eq!(rows[0]["active_binding_count"], 0);
        assert_eq!(rows[2]["active_binding_count"], 1);
        assert_eq!(rows[0]["source"], "builtin");
        assert_eq!(rows[0]["color"], "#3b82f6");
        // Tenant isolation + tombstone scoping exclusions.
        let excluded = fixture["rows_excluded"].as_array().expect("excluded");
        assert!(excluded
            .iter()
            .any(|r| r["why"] == "schedulers.deleted_at IS NULL"));
        assert!(excluded.iter().any(|r| r["why"] == "workspace_id scope"));
        assert!(excluded
            .iter()
            .any(|r| r["why"] == "pk + workspace__slug scope (R2)"));
        // Delete cascade effects (ported quirk 4: updated_at untouched).
        assert!(fixture["delete_cascade"]["after"]["bindings_rows"]
            .as_str()
            .unwrap_or("")
            .contains("updated_at untouched"));
    }

    #[test]
    fn binding_rows_and_rules_match_f36_05() {
        let fixture = fixture_json(FIXTURE_BINDING_ROWS);
        assert_eq!(fixture["source"], "app/views/scheduler/views.py:163-291");
        let rows = fixture["rows"].as_array().expect("rows array");
        assert_eq!(rows.len(), 2);
        // Newest first; joined scheduler/runner wire fields populated.
        assert_eq!(rows[0]["scheduler_slug"], "second");
        assert_eq!(rows[0]["rrule"], "FREQ=HOURLY");
        assert_eq!(rows[1]["scheduler_slug"], "first");
        assert_eq!(rows[1]["last_run_status"], "completed");
        assert_eq!(rows[1]["pod_name"], "runner-pod");
        // The recompute rules pin this module's decision API.
        let rules = &fixture["next_run_at_rules"];
        let trigger: Vec<&str> = rules["patch_recompute_trigger_keys"]
            .as_array()
            .expect("trigger keys")
            .iter()
            .map(|k| k.as_str().expect("key"))
            .collect();
        assert_eq!(trigger, RECOMPUTE_TRIGGER_KEYS);
        assert!(rules["create_writes_when"]
            .as_str()
            .unwrap_or("")
            .contains("AND"));
        assert!(rules["patch_writes_when"]
            .as_str()
            .unwrap_or("")
            .contains("NO comparison"));
        assert!(rules["patch_refresh"]
            .as_str()
            .unwrap_or("")
            .contains("refresh_from_db"));
        // Tenant isolation exclusions.
        let excluded = fixture["rows_excluded"].as_array().expect("excluded");
        assert!(excluded
            .iter()
            .any(|r| r["why"] == "scheduler_bindings.deleted_at IS NULL"));
        assert!(excluded.iter().any(|r| r["why"] == "project_id scope"));
    }

    #[test]
    fn pod_and_lastrun_columns_match_f36_06() {
        let fixture = fixture_json(FIXTURE_POD_LASTRUN);
        let pod_cols: Vec<&str> = fixture["pod"]["columns"]
            .as_array()
            .expect("pod columns")
            .iter()
            .map(|c| c.as_str().expect("column"))
            .collect();
        assert_eq!(pod_cols, POD_COLUMNS);
        assert_eq!(fixture["pod"]["db_table"], POD_TABLE);
        let run_cols: Vec<&str> = fixture["agent_run"]["columns"]
            .as_array()
            .expect("agent_run columns")
            .iter()
            .map(|c| c.as_str().expect("column"))
            .collect();
        assert_eq!(run_cols, AGENT_RUN_COLUMNS);
        assert_eq!(fixture["agent_run"]["db_table"], AGENT_RUN_TABLE);
        // agent_run has no deleted_at column (not a SoftDeleteModel).
        assert!(!AGENT_RUN_COLUMNS.contains(&"deleted_at"));
        // The D-36 reads off each table.
        assert_eq!(
            fixture["pod"]["d36_reads"]["binding_list_R1_via_pod"]
                .as_array()
                .expect("pod reads")[0],
            "name -> pod_name"
        );
        // Row structs serialize to exactly the pinned columns.
        let pod = PodRow {
            id: uuid::Uuid::nil(),
            workspace_id: uuid::Uuid::nil(),
            project_id: uuid::Uuid::nil(),
            name: String::new(),
            description: String::new(),
            created_by_id: None,
            is_default: false,
            deleted_at: None,
            created_at: DateTime::from_timestamp(0, 0).expect("epoch"),
            updated_at: DateTime::from_timestamp(0, 0).expect("epoch"),
        };
        let mut pod_keys: Vec<String> = serde_json::to_value(&pod)
            .expect("pod serializes")
            .as_object()
            .expect("pod object")
            .keys()
            .cloned()
            .collect();
        pod_keys.sort();
        let mut expected_pod: Vec<String> = POD_COLUMNS.iter().map(|c| (*c).to_owned()).collect();
        expected_pod.sort();
        assert_eq!(pod_keys, expected_pod);
    }

    #[test]
    fn patch_trigger_is_key_presence_not_value_change() {
        assert_eq!(
            RECOMPUTE_TRIGGER_KEYS,
            &["dtstart", "rrule", "rdates", "exdates", "tzid"]
        );
        for &key in RECOMPUTE_TRIGGER_KEYS {
            let body = serde_json::json!({ key: "anything" });
            assert!(patch_triggers_recompute(&body), "{key} triggers");
            // Explicit null still triggers (Python `in` on request.data).
            let null_body = serde_json::json!({ key: None::<()> });
            assert!(patch_triggers_recompute(&null_body), "{key}=null triggers");
        }
        assert!(!patch_triggers_recompute(
            &serde_json::json!({"enabled": false})
        ));
        assert!(!patch_triggers_recompute(&serde_json::json!({})));
        assert!(!patch_triggers_recompute(&serde_json::json!([])));
    }

    #[test]
    fn install_and_patch_write_decisions_keep_the_asymmetry() {
        let t1 = DateTime::from_timestamp(100, 0).expect("t1");
        let t2 = DateTime::from_timestamp(200, 0).expect("t2");
        // Install (:218): non-null AND differs from stored.
        assert_eq!(install_next_run_at_decision(Some(t2), None), Some(t2));
        assert_eq!(install_next_run_at_decision(Some(t2), Some(t1)), Some(t2));
        assert_eq!(install_next_run_at_decision(Some(t1), Some(t1)), None);
        assert_eq!(install_next_run_at_decision(None, None), None);
        assert_eq!(install_next_run_at_decision(None, Some(t1)), None);
        // Patch (:272): non-null, no comparison — even when identical.
        assert_eq!(patch_next_run_at_decision(Some(t1)), Some(t1));
        assert_eq!(patch_next_run_at_decision(None), None);
    }

    #[test]
    fn closure_helpers_forward_inputs_and_apply_decisions() {
        let dtstart = DateTime::from_timestamp(1000, 0).expect("dtstart");
        let now = DateTime::from_timestamp(2000, 0).expect("now");
        let fire = DateTime::from_timestamp(3000, 0).expect("fire");
        let rdates = serde_json::json!(["2024-05-01T00:00:00+00:00"]);
        let exdates = serde_json::json!([]);
        let bundle = RruleBundle {
            dtstart,
            rrule: "FREQ=HOURLY",
            tzid: "UTC",
            rdates: &rdates,
            exdates: &exdates,
        };
        // The closure receives the exact bundle + now.
        let out = install_next_run_at(&bundle, None, now, |got, got_now| {
            assert_eq!(*got, bundle);
            assert_eq!(got_now, now);
            Some(fire)
        });
        assert_eq!(out, Some(fire));
        // Install applies the differs-from-stored rule.
        let out = install_next_run_at(&bundle, Some(fire), now, |_, _| Some(fire));
        assert_eq!(out, None);
        // Patch applies the non-null rule (no comparison).
        let out = patch_next_run_at(&bundle, now, |_, _| Some(fire));
        assert_eq!(out, Some(fire));
        let out = patch_next_run_at(&bundle, now, |_, _| None);
        assert_eq!(out, None);
    }

    #[test]
    fn joined_rows_decode_with_nullable_legs() {
        let scheduler_json = serde_json::json!({
            "id": "11111111-1111-1111-1111-111111111111",
            "created_at": "2024-05-01T00:00:00Z",
            "updated_at": "2024-05-01T00:00:00Z",
            "created_by_id": None::<String>,
            "updated_by_id": None::<String>,
            "deleted_at": None::<String>,
            "workspace_id": "22222222-2222-2222-2222-222222222222",
            "slug": "nightly",
            "name": "Nightly",
            "description": "",
            "prompt": "do things",
            "source": "builtin",
            "is_enabled": true,
            "color": "#3b82f6"
        });
        let scheduler: scheduler::Scheduler =
            serde_json::from_value(scheduler_json).expect("scheduler decodes");
        let row = SchedulerListRow {
            scheduler,
            active_binding_count: 2,
        };
        assert_eq!(row.active_binding_count, 2);
        assert_eq!(row.scheduler.slug, "nightly");
        let roundtrip: SchedulerListRow =
            serde_json::from_value(serde_json::to_value(&row).expect("row serializes"))
                .expect("row round-trips");
        assert_eq!(roundtrip, row);

        let binding_json = serde_json::json!({
            "id": "33333333-3333-3333-3333-333333333333",
            "created_at": "2024-05-02T00:00:00Z",
            "updated_at": "2024-05-02T00:00:00Z",
            "created_by_id": "44444444-4444-4444-4444-444444444444",
            "updated_by_id": None::<String>,
            "deleted_at": None::<String>,
            "workspace_id": "22222222-2222-2222-2222-222222222222",
            "project_id": "55555555-5555-5555-5555-555555555555",
            "scheduler_id": "11111111-1111-1111-1111-111111111111",
            "dtstart": "2024-05-01T00:00:00Z",
            "tzid": "UTC",
            "rrule": "FREQ=HOURLY",
            "rdates": [],
            "exdates": [],
            "extra_context": "",
            "enabled": true,
            "outcome_mode": "create_issue",
            "next_run_at": "2024-05-07T07:00:00Z",
            "last_run_id": None::<String>,
            "last_error": "",
            "actor_id": "44444444-4444-4444-4444-444444444444",
            "pod_id": None::<String>
        });
        let binding: scheduler_binding::SchedulerBinding =
            serde_json::from_value(binding_json).expect("binding decodes");
        let joined = BindingListRow {
            binding,
            scheduler: row.scheduler.clone(),
            last_run: None,
            pod: None,
        };
        // NULL FKs decode to absent legs (LEFT JOIN all-NULL).
        assert!(joined.last_run.is_none());
        assert!(joined.pod.is_none());
        assert_eq!(joined.scheduler.slug, "nightly");
        let roundtrip: BindingListRow =
            serde_json::from_value(serde_json::to_value(&joined).expect("joined serializes"))
                .expect("joined round-trips");
        assert_eq!(roundtrip, joined);
    }
}
