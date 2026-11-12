//! `POST /api/runners/runs/` dispatch tails (PIDASHCONV-1041).
//!
//! Ports the three dispatch tails of `AgentRunListEndpoint.post`
//! (`runner/views/runs.py:221-466`) that [`create_run`](super::runs::create_run)
//! used to proxy to Django:
//!
//! * direct (`:238-345`) → [`post_direct`]: `validate_run_creation`
//!   (§6.5) over a pool-backed [`PoolRunCreationStore`], the
//!   `execution_fields` + `AgentRun` insert + `build_direct_turn` +
//!   prompt-cap + `dispatch_after_commit` transaction, then the
//!   `AgentRunSerializer` 201.
//! * `run_ai` (`:347-388`) → [`post_run_ai`] and `comment_and_run`
//!   (`:420-466`) → [`post_comment_and_run`]: the issue-bound prefix
//!   (falsy `work_item` 400, unknown issue / non-member 404) plus the
//!   single-transaction human clock (`reconcile` with
//!   `want_run=true`, 202 when the entry queues) and the D-12 dispatch,
//!   rolled back when the dispatch yields no run.
//!
//! Composition only: every tail drives the already-ported services/jobs
//! units through their public API (`validation`, `dispatch`,
//! `execution_fields`, `dispatch_agent_run`, the `DispatchSeam`/`CreationSeam`
//! traits over [`LiveCreationStore`](pidash_jobs::orchestration::LiveCreationStore),
//! `clock::reconcile`, the prompting `build_*` assemblers). No foundation
//! file is touched; the D-15-owned pieces here are the request parsing
//! (Django `filter()` UUID coercion, probed), the direct INSERT, and the
//! web-only clock+dispatch transaction shape.
//!
//! Still proxied by `create_run`: the malformed-JSON `ParseError` arm and
//! the non-object arm — both need Django's parsers byte-for-byte (DRF
//! messages carry CPython `json` wording/positions), so they stay
//! proxy-before-decide like every other handler's parser arms.

use axum::http::StatusCode;
use axum::response::Response;
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, EntityTrait, FromQueryResult, JoinType, QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::entity as orm;
use pidash_db::orm_tx::TxConn;
use pidash_db::pool::sea_orm_connection;
use pidash_db::redis::client_from_settings;
use pidash_db::runner_runs::pod::Pod;
use pidash_db::runner_runs::AgentRun;
use pidash_db::tx::Transaction as DbTransaction;
use pidash_jobs::dispatch::{
    dispatch_after_commit as collect_dispatch, dispatch_agent_run,
    execution_fields as resolve_execution_fields, ActorScope, DispatchDecision,
    ExecutionFieldsError, ExecutionInputs, ExecutionSeams, ProjectScope,
};
use pidash_jobs::orchestration::LiveCreationStore;
use pidash_services::dispatch::policy::{resolve_executor_kind, UserFlags};
use pidash_services::dispatch::{consume_admission_token, DeferredConsume};
use pidash_services::orchestration::clock::{self, ClockIssue, ClockWrite};
use pidash_services::orchestration::creation::{parent_done_payload, tick_value, CreationSeam};
use pidash_services::orchestration::dispatch::DispatchSeam;
use pidash_services::prompting::context as prompting_context;
use pidash_services::runner_enroll::validation::{
    self, IssueProbe, RunCreationError, RunCreationStore, StoreError, ValidatedRunContext,
    ValidationError,
};
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::orchestration::{
    is_ticking_state, StateRef, TickerEvent, TRIGGER_COMMENT_AND_RUN, TRIGGER_RUN_AI,
};
use pidash_types::runner_runs::AgentRunStatus;

use super::runs::{
    binding_detail_for, fetch_guarded_run, is_workspace_member, issue_lookup, parse_py_uuid,
    pod_mini_for, py_truthy_json, render_detail, run_from_model, tool_calls_for, workspace_role,
    ISSUE_NOT_FOUND_BODY,
};
use super::{json_response, server_error};
use crate::project_move_handoff::RedisAdmissionCache;
use crate::state::AppState;
use crate::v1_work_items::handlers_actions::{dispatch_deps, draw_jitter, fetch_project, IssueRow};

// ---------------------------------------------------------------------------
// Exact bodies (only arms this port answers)
// ---------------------------------------------------------------------------

/// `run_ai` dispatch yielded no run (`runs.py:380-384`).
pub const RUN_AI_DISPATCH_FAILED_BODY: &str = r#"{"error":"could not dispatch — issue may already have an active run, or no pod is available"}"#;
/// `comment_and_run` dispatch yielded no run (`runs.py:454-462`).
pub const COMMENT_RUN_DISPATCH_FAILED_BODY: &str = r#"{"error":"could not dispatch — issue may already have an active run, or no prior run / pod is available"}"#;
/// Direct-create lost the active-run race (`runs.py:335-339`).
pub const ACTIVE_RUN_EXISTS_BODY: &str =
    r#"{"error":"work item already has an active run","code":"active_run_exists"}"#;
/// Cloud prompt over the execution limit (`runs.py:330-334`).
pub const PROMPT_TOO_LARGE_BODY: &str =
    r#"{"error":"prompt exceeds Cloud Agent limit","code":"prompt_too_large"}"#;
/// Queued-entry detail (`runs.py:410-417`): the `reason` key varies, the
/// `detail` sentence is fixed.
pub const QUEUED_ENTRY_DETAIL: &str =
    "a run is active on this work item; the next run is queued and starts when it ends";

/// `run_kind` stamped on direct dispatches (`runs.py:266`).
const RUN_KIND_DIRECT: &str = "direct";
/// `AgentRun.trigger` model default (`runner/models.py:965-969`): the
/// direct create passes no trigger, so every direct run carries it.
const TRIGGER_DIRECT: &str = "direct";

// ---------------------------------------------------------------------------
// Django `filter()` UUID coercion (probed, PIDASHCONV-1041)
// ---------------------------------------------------------------------------

/// What one `filter(pk=…)` / `filter(workspace_id=…)` / `filter(pod=…)`
/// coercion decided. Django's `UUIDField.get_prep_value` takes
/// `uuid.UUID(int=value)` for integers (bools included — `isinstance(True,
/// int)`) and `uuid.UUID(hex=value)` for strings; anything else raises
/// (`ValidationError` for bad strings, `TypeError`/`AttributeError` for
/// floats and containers) and Django answers 500.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UuidCoerce {
    /// JSON missing/null: the `is not None` / falsy checks see no value.
    Absent,
    /// Coercion raised: the handler's 500.
    Invalid,
    /// Coerced id plus the raw rendering (`str(value)` in Python — the
    /// `str() != str()` consistency comparisons in `validation.py:87-92`,
    /// `:134-143` read the raw text, not the canonical form, so a
    /// non-canonical workspace fails every comparison it reaches).
    Id {
        /// The coerced id (query bindings).
        id: Uuid,
        /// `str(value)`: strings verbatim, integers decimal, bools
        /// `True`/`False` (the comparisons' right-hand side).
        raw: String,
    },
}

/// Coerce one raw request value the way `filter()` would.
pub fn coerce_filter_uuid(value: Option<&Value>) -> UuidCoerce {
    match value {
        None | Some(Value::Null) => UuidCoerce::Absent,
        Some(Value::String(text)) => match parse_py_uuid(text) {
            Some(id) => UuidCoerce::Id {
                id,
                raw: text.clone(),
            },
            None => UuidCoerce::Invalid,
        },
        // `uuid.UUID(int=value)`: 0 <= value < 2^128, else `ValueError`.
        // JSON integers past `u64` still coerce (Python ints are
        // unbounded), so pure-digit text parses as `u128` directly.
        Some(Value::Number(number)) => {
            if let Some(int) = number.as_i64() {
                if int >= 0 {
                    UuidCoerce::Id {
                        id: Uuid::from_u128(int as u128),
                        raw: number.to_string(),
                    }
                } else {
                    UuidCoerce::Invalid
                }
            } else if let Some(uint) = number.as_u64() {
                UuidCoerce::Id {
                    id: Uuid::from_u128(u128::from(uint)),
                    raw: number.to_string(),
                }
            } else if number.to_string().bytes().all(|b| b.is_ascii_digit()) {
                match number.to_string().parse::<u128>() {
                    Ok(wide) => UuidCoerce::Id {
                        id: Uuid::from_u128(wide),
                        raw: number.to_string(),
                    },
                    // >= 2^128: `ValueError`.
                    Err(_) => UuidCoerce::Invalid,
                }
            } else {
                // Floats take the `hex=` path and raise (`AttributeError`).
                UuidCoerce::Invalid
            }
        }
        Some(Value::Bool(flag)) => UuidCoerce::Id {
            id: Uuid::from_u128(u128::from(*flag as u8)),
            raw: if *flag {
                "True".to_owned()
            } else {
                "False".to_owned()
            },
        },
        // Lists and dicts raise `TypeError`.
        Some(Value::Array(_)) | Some(Value::Object(_)) => UuidCoerce::Invalid,
    }
}

/// Truncate to microseconds: Postgres `timestamptz` and Python datetimes
/// both stop there, and `Utc::now()` carries nanos.
fn now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).unwrap_or(now)
}

// ---------------------------------------------------------------------------
// `RunCreationStore` over the pool (D-15 consumer, PIDASHCONV-1041)
// ---------------------------------------------------------------------------

/// The §6.5 reads over a pool handle: the validation `*_SQL` consts'
/// statements as SeaORM selects, in the order [`validate_run_creation`]
/// issues them (same scopes, ordering and `LIMIT`s).
pub struct PoolRunCreationStore<'a> {
    pool: &'a PgPool,
}

impl<'a> PoolRunCreationStore<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        PoolRunCreationStore { pool }
    }

    fn db(error: sea_orm::DbErr) -> StoreError {
        StoreError::Db(pidash_db::orm_tx::into_sqlx(error).to_string())
    }
}

/// The L2 [`Pod`] off a SeaORM model.
fn pod_from_model(model: orm::pod::Model) -> Pod {
    Pod {
        id: model.id,
        workspace_id: model.workspace_id,
        project_id: model.project_id,
        name: model.name,
        description: model.description,
        created_by_id: model.created_by_id,
        is_default: model.is_default,
        deleted_at: model.deleted_at.map(|at| at.with_timezone(&Utc)),
        created_at: model.created_at.with_timezone(&Utc),
        updated_at: model.updated_at.with_timezone(&Utc),
    }
}

/// `Pod.objects` (the `PodManager` scope: soft-deleted pods read as
/// missing) with the model ordering (`-is_default`, `created_at`),
/// first row ([`validation::POD_BY_ID_SQL`] /
/// [`validation::DEFAULT_POD_FOR_PROJECT_SQL`]).
fn pod_scope() -> sea_orm::Select<orm::pod::Entity> {
    use orm::pod::{Column, Entity};
    Entity::find()
        .filter(Column::DeletedAt.is_null())
        .order_by_desc(Column::IsDefault)
        .order_by_asc(Column::CreatedAt)
        .limit(1)
}

impl RunCreationStore for PoolRunCreationStore<'_> {
    async fn workspace_role(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<Option<i32>, StoreError> {
        use orm::workspace_members::{Column, Entity};
        // `role` is a `smallint`: decode `i16`, widen to the trait's `i32`.
        // [`validation::WORKSPACE_ROLE_SQL`]: live + active, `-created_at`.
        let role = Entity::find()
            .select_only()
            .column(Column::Role)
            .filter(Column::DeletedAt.is_null())
            .filter(Column::IsActive.eq(true))
            .filter(Column::MemberId.eq(user_id))
            .filter(Column::WorkspaceId.eq(workspace_id))
            .order_by_desc(Column::CreatedAt)
            .limit(1)
            .into_tuple::<i16>()
            .one(&sea_orm_connection(self.pool))
            .await
            .map_err(Self::db)?;
        Ok(role.map(i32::from))
    }

    async fn issue_probe(&self, work_item_id: Uuid) -> Result<Option<IssueProbe>, StoreError> {
        use orm::issues::{Column, Entity};
        // [`validation::ISSUE_PROBE_SQL`]: the plain soft-delete scope,
        // `-created_at`, first row.
        let row = Entity::find()
            .select_only()
            .columns([
                Column::WorkspaceId,
                Column::AssignedPodId,
                Column::ProjectId,
            ])
            .filter(Column::DeletedAt.is_null())
            .filter(Column::Id.eq(work_item_id))
            .order_by_desc(Column::CreatedAt)
            .limit(1)
            .into_tuple::<(Uuid, Option<Uuid>, Uuid)>()
            .one(&sea_orm_connection(self.pool))
            .await
            .map_err(Self::db)?;
        Ok(
            row.map(|(workspace_id, assigned_pod_id, project_id)| IssueProbe {
                workspace_id,
                assigned_pod_id,
                project_id: Some(project_id),
            }),
        )
    }

    async fn pod_by_id(&self, pod_id: Uuid) -> Result<Option<Pod>, StoreError> {
        Ok(pod_scope()
            .filter(orm::pod::Column::Id.eq(pod_id))
            .one(&sea_orm_connection(self.pool))
            .await
            .map_err(Self::db)?
            .map(pod_from_model))
    }

    async fn default_pod_for_project(&self, project_id: Uuid) -> Result<Option<Pod>, StoreError> {
        Ok(pod_scope()
            .filter(orm::pod::Column::IsDefault.eq(true))
            .filter(orm::pod::Column::ProjectId.eq(project_id))
            .one(&sea_orm_connection(self.pool))
            .await
            .map_err(Self::db)?
            .map(pod_from_model))
    }

    async fn project_ids_for_workspace(&self, workspace_id: Uuid) -> Result<Vec<Uuid>, StoreError> {
        use orm::projects::{Column, Entity};
        // [`validation::PROJECT_IDS_FOR_WORKSPACE_SQL`]: soft-delete scope,
        // `-created_at`, the first two.
        Entity::find()
            .select_only()
            .column(Column::Id)
            .filter(Column::DeletedAt.is_null())
            .filter(Column::WorkspaceId.eq(workspace_id))
            .order_by_desc(Column::CreatedAt)
            .limit(2)
            .into_tuple::<Uuid>()
            .all(&sea_orm_connection(self.pool))
            .await
            .map_err(Self::db)
    }
}

// ---------------------------------------------------------------------------
// Direct create (`runs.py:238-345`)
// ---------------------------------------------------------------------------

/// The direct-create `INSERT`: Django's `AgentRun.objects.create` at
/// `runs.py:281-292` inserts every concrete column (model order, the
/// three generated `usage` columns and the generated `refusal_category`
/// excluded), so the active model sets them all with the model defaults;
/// `trigger` is the `'direct'` model default and `phase_kind` stays `''`
/// (the view passes neither — the `phase_kind` stamp is an
/// orchestration-creation behavior, not a direct one).
struct DirectRunInsert<'a> {
    run_id: Uuid,
    workspace_id: Uuid,
    created_by_id: Uuid,
    pod_id: Uuid,
    pinned_runner_id: Option<Uuid>,
    /// `AgentRun.pin_kind` (runner 0039, design §6.4).
    pin_kind: &'a str,
    work_item_id: Option<Uuid>,
    executor_kind: &'a str,
    error_details: &'a Value,
    tool_plan: &'a Value,
    run_config: &'a Value,
    required_capabilities: &'a Value,
    created_at: DateTime<Utc>,
}

impl DirectRunInsert<'_> {
    fn active_model(&self) -> orm::agent_run::ActiveModel {
        orm::agent_run::ActiveModel {
            id: Set(self.run_id),
            status: Set(AgentRunStatus::Queued.value().to_owned()),
            prompt: Set(String::new()),
            run_config: Set(self.run_config.clone()),
            required_capabilities: Set(self.required_capabilities.clone()),
            thread_id: Set(String::new()),
            lease_expires_at: Set(None),
            done_payload: Set(None),
            created_at: Set(self.created_at.into()),
            assigned_at: Set(None),
            started_at: Set(None),
            ended_at: Set(None),
            owner_id: Set(None),
            workspace_id: Set(self.workspace_id),
            runner_id: Set(None),
            work_item_id: Set(self.work_item_id),
            parent_run_id: Set(None),
            pod_id: Set(self.pod_id),
            created_by_id: Set(self.created_by_id),
            pinned_runner_id: Set(self.pinned_runner_id),
            pin_kind: Set(self.pin_kind.to_owned()),
            scheduler_binding_id: Set(None),
            llm_model: Set(String::new()),
            trigger: Set(TRIGGER_DIRECT.to_owned()),
            prompt_manifest: Set(None),
            executor_kind: Set(self.executor_kind.to_owned()),
            dispatch_attempts: Set(0),
            cancel_requested_at: Set(None),
            cancel_reason: Set(String::new()),
            tool_plan: Set(self.tool_plan.clone()),
            terminal_hooks_applied_at: Set(None),
            terminal_capacity_released_at: Set(None),
            phase_kind: Set(String::new()),
            agent_metadata: Set(Value::Object(Default::default())),
            usage: Set(Value::Object(Default::default())),
            error_details: Set(self.error_details.clone()),
            // Generated columns take their defaults.
            input_tokens: sea_orm::NotSet,
            output_tokens: sea_orm::NotSet,
            total_tokens: sea_orm::NotSet,
            refusal_category: sea_orm::NotSet,
        }
    }
}

/// The `requested` override read (`runs.py:273-279`):
/// `Issue.objects.filter(pk).values_list("agent_executor").first()` —
/// the default manager, so a soft-deleted issue reads as `None`.
fn direct_requested_select(
    work_item_id: Uuid,
) -> sea_orm::Selector<sea_orm::SelectGetableTuple<Option<String>>> {
    use orm::issues::{Column, Entity};
    Entity::find()
        .select_only()
        .column(Column::AgentExecutor)
        .filter(Column::DeletedAt.is_null())
        .filter(Column::Id.eq(work_item_id))
        .limit(1)
        .into_tuple::<Option<String>>()
}

/// The execution-scope project row (`ctx.pod.project`, `runs.py:265`):
/// `(id, workspace_id, default_agent_executor)`.
fn direct_project_select(
    project_id: Uuid,
) -> sea_orm::Selector<sea_orm::SelectGetableTuple<(Uuid, Uuid, String)>> {
    use orm::projects::{Column, Entity};
    Entity::find()
        .select_only()
        .columns([
            Column::Id,
            Column::WorkspaceId,
            Column::DefaultAgentExecutor,
        ])
        .filter(Column::Id.eq(project_id))
        .limit(1)
        .into_tuple::<(Uuid, Uuid, String)>()
}

/// The creator flags (`actor`, `runs.py:269/271`): `(is_active, is_bot)`.
fn direct_actor_select(
    user_id: Uuid,
) -> sea_orm::Selector<sea_orm::SelectGetableTuple<(bool, bool)>> {
    use orm::users::{Column, Entity};
    Entity::find()
        .select_only()
        .columns([Column::IsActive, Column::IsBot])
        .filter(Column::Id.eq(user_id))
        .limit(1)
        .into_tuple::<(bool, bool)>()
}

/// The creator's BYOK key presence (`user_has_llm_config`,
/// `agent_execution.py:69-80`, via the assistant gate's read).
fn direct_llm_config_select(
    user_id: Uuid,
) -> sea_orm::Selector<sea_orm::SelectGetableTuple<Option<Vec<u8>>>> {
    use orm::assistant_user_llm_config::{Column, Entity};
    Entity::find()
        .select_only()
        .column(Column::ApiKeyEncrypted)
        .filter(Column::UserId.eq(user_id))
        .limit(1)
        .into_tuple::<Option<Vec<u8>>>()
}

/// The free-form cloud-context workspace row (`composer.py:477`):
/// `(slug, name)`.
fn direct_workspace_select(
    workspace_id: Uuid,
) -> sea_orm::Selector<sea_orm::SelectGetableTuple<(String, String)>> {
    use orm::workspaces::{Column, Entity};
    Entity::find()
        .select_only()
        .columns([Column::Slug, Column::Name])
        .filter(Column::Id.eq(workspace_id))
        .limit(1)
        .into_tuple::<(String, String)>()
}

/// One free-form cloud-context project row: `(id, identifier, name,
/// description)`.
type ContextProjectRow = (Uuid, String, String, Option<String>);

/// The free-form cloud-context project row (`composer.py:476`).
fn direct_context_project_select(
    project_id: Uuid,
) -> sea_orm::Selector<sea_orm::SelectGetableTuple<ContextProjectRow>> {
    use orm::projects::{Column, Entity};
    Entity::find()
        .select_only()
        .columns([
            Column::Id,
            Column::Identifier,
            Column::Name,
            Column::Description,
        ])
        .filter(Column::Id.eq(project_id))
        .limit(1)
        .into_tuple::<ContextProjectRow>()
}

/// The v1 `IssueRow` columns ([`dispatch_issue_select`]).
#[derive(Debug, Clone, PartialEq, Eq, FromQueryResult)]
struct DispatchIssueRow {
    id: Uuid,
    project_id: Uuid,
    workspace_id: Uuid,
    created_by_id: Option<Uuid>,
    agent_executor: Option<String>,
    state_id: Option<Uuid>,
    state_name: Option<String>,
    state_group: Option<String>,
}

impl DispatchIssueRow {
    fn into_issue_row(self) -> IssueRow {
        IssueRow {
            id: self.id,
            project_id: Some(self.project_id),
            workspace_id: self.workspace_id,
            created_by_id: self.created_by_id,
            agent_executor: self.agent_executor,
            state_id: self.state_id,
            state_name: self.state_name,
            state_group: self.state_group,
        }
    }
}

/// The v1 `IssueRow` for the web dispatch tails: the issue plus its
/// state names (`LEFT JOIN states`), `all_objects` scope (no deleted
/// guard — `runs.py:365/437`), by id.
fn dispatch_issue_select(
    issue_id: Uuid,
) -> sea_orm::Selector<sea_orm::SelectModel<DispatchIssueRow>> {
    use orm::issues::{Column, Entity};
    Entity::find()
        .select_only()
        .columns([
            Column::Id,
            Column::ProjectId,
            Column::WorkspaceId,
            Column::CreatedById,
            Column::AgentExecutor,
            Column::StateId,
        ])
        .column_as(orm::states::Column::Name, "state_name")
        .column_as(orm::states::Column::Group, "state_group")
        .join(
            JoinType::LeftJoin,
            Entity::belongs_to(orm::states::Entity)
                .from(Column::StateId)
                .to(orm::states::Column::Id)
                .into(),
        )
        .filter(Column::Id.eq(issue_id))
        .limit(1)
        .into_model::<DispatchIssueRow>()
}

/// `run.save(update_fields=["prompt", "prompt_manifest"])`
/// (`orchestration::creation::PROMPT_UPDATE_SQL`): the rendered prompt
/// and its manifest stamp (`NULL` without one).
fn prompt_update_statement(
    run_id: Uuid,
    prompt: &str,
    manifest: Option<&Value>,
) -> sea_orm::UpdateMany<orm::agent_run::Entity> {
    use orm::agent_run::{Column, Entity};
    Entity::update_many()
        .col_expr(Column::Prompt, Expr::value(prompt.to_owned()))
        .col_expr(Column::PromptManifest, Expr::value(manifest.cloned()))
        .filter(Column::Id.eq(run_id))
}

/// Ordered `{"error","code"}` (struct order, not `json!` — maps sort keys).
#[derive(serde::Serialize)]
struct ErrorCode<'a> {
    error: &'a str,
    code: &'a str,
}

/// Ordered 429 (`runs.py:313-318`).
#[derive(serde::Serialize)]
struct Throttled<'a> {
    error: &'a str,
    code: &'a str,
    retry_after_seconds: i64,
}

/// Ordered 202 (`runs.py:410-417`).
#[derive(serde::Serialize)]
struct QueuedEntry<'a> {
    queued: bool,
    reason: &'a str,
    detail: &'a str,
}

/// Render the `{"error","code"}` body of a §6.5 failure at its status
/// (`runs.py:253-257`). (Struct order keeps `error` first.)
fn run_creation_denial(error: &RunCreationError) -> Response {
    let status = StatusCode::from_u16(error.status()).unwrap_or(StatusCode::BAD_REQUEST);
    let body = serde_json::to_value(&ErrorCode {
        error: error.message(),
        code: error.code(),
    })
    .expect("serializable denial")
    .to_string();
    json_response(status, body)
}

/// Map one `execution_fields` failure onto the direct-create arms
/// (`runs.py:310-329`): `code = getattr(exc, "code", str(exc))`, then
/// quota → 429, admission-down → 503, capability → 422, else 409.
/// The transaction is already rolled back by the caller.
fn execution_fields_denial(error: &ExecutionFieldsError) -> Response {
    let quota = pidash_services::dispatch::admission::CloudAgentAdmissionError::RUN_QUOTA_EXCEEDED;
    let down =
        pidash_services::dispatch::admission::CloudAgentAdmissionError::ADMISSION_UNAVAILABLE;
    match error {
        ExecutionFieldsError::Admission(admission) => {
            let code = admission.code().to_owned();
            let detail = admission.detail().to_owned();
            if code == quota {
                match admission.retry_after_seconds() {
                    Some(retry) => {
                        let body = serde_json::to_value(&Throttled {
                            error: &detail,
                            code: &code,
                            retry_after_seconds: retry.max(1),
                        })
                        .expect("serializable throttle")
                        .to_string();
                        json_response(StatusCode::TOO_MANY_REQUESTS, body)
                    }
                    // `max(1, None)` raises `TypeError` in Python — a 500.
                    None => server_error(),
                }
            } else if code == down {
                let body = serde_json::to_value(&ErrorCode {
                    error: &detail,
                    code: &code,
                })
                .expect("serializable denial")
                .to_string();
                json_response(StatusCode::SERVICE_UNAVAILABLE, body)
            } else {
                let body = serde_json::to_value(&ErrorCode {
                    error: &detail,
                    code: &code,
                })
                .expect("serializable denial")
                .to_string();
                json_response(StatusCode::CONFLICT, body)
            }
        }
        ExecutionFieldsError::Capability(capability) => {
            let body = serde_json::to_value(&ErrorCode {
                error: capability.message(),
                code: capability.code(),
            })
            .expect("serializable denial")
            .to_string();
            json_response(StatusCode::UNPROCESSABLE_ENTITY, body)
        }
        ExecutionFieldsError::Unavailable(unavailable) => {
            let body = serde_json::to_value(&ErrorCode {
                error: unavailable.message(),
                code: unavailable.code(),
            })
            .expect("serializable denial")
            .to_string();
            json_response(StatusCode::CONFLICT, body)
        }
        ExecutionFieldsError::Resolve(resolve) => {
            let (error, code) = match resolve {
                pidash_services::dispatch::policy::ResolveExecutorError::UnknownExecutor => (
                    "unknown agent executor".to_owned(),
                    "unknown agent executor".to_owned(),
                ),
                pidash_services::dispatch::policy::ResolveExecutorError::CloudAgent(
                    unavailable,
                ) => (
                    unavailable.message().to_owned(),
                    unavailable.code().to_owned(),
                ),
            };
            let body = serde_json::to_value(&ErrorCode {
                error: &error,
                code: &code,
            })
            .expect("serializable denial")
            .to_string();
            json_response(StatusCode::CONFLICT, body)
        }
        ExecutionFieldsError::Db(_) => server_error(),
    }
}

/// Convert `required_capabilities` for the cloud tool planner
/// (`policy.py:97-134`): `set(...)` over the raw value. Only the cloud
/// path inspects it — local/managed runs store the value verbatim and
/// never look — so this runs only past a cloud resolution. `Err` is the
/// `TypeError` Django raises (a 500): non-string members break the
/// `sorted`/`join`, non-iterables break `set()` itself. Strings iterate
/// to chars, dicts to keys, exactly like `set()`.
fn convert_capabilities(raw: &Value) -> Result<Vec<String>, ()> {
    if !py_truthy_json(Some(raw)) {
        return Ok(Vec::new());
    }
    match raw {
        Value::String(text) => Ok(text.chars().map(|c| c.to_string()).collect()),
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(text) => out.push(text.clone()),
                    _ => return Err(()),
                }
            }
            Ok(out)
        }
        Value::Object(map) => Ok(map.keys().cloned().collect()),
        _ => Err(()),
    }
}

/// Roll the direct-create transaction back, then answer `response` —
/// unless the rollback itself failed, which is a 500 either way (the
/// rollback exception would propagate out of Django's `atomic` too).
async fn rollback_then(tx: DbTransaction<'_>, response: Response) -> Response {
    match tx.rollback().await {
        Ok(()) => response,
        Err(_) => server_error(),
    }
}

/// Drain post-commit dispatch: deferred admission consumes first, then
/// one `dispatch_agent_run` per collected run — Django's `on_commit`
/// registration order (the creation-rate consumes register inside
/// `execution_fields`, before the `dispatch_after_commit` hook at
/// `runs.py:309`). A [`DispatchDecision::PodDrain`] drains through
/// `drain_pod_by_id` — Python drains the pod inline
/// (`dispatch.py:61-70`), so the decision is acted on, never dropped.
/// A drain failure is a 500 with the rows committed, exactly like an
/// `on_commit` raise.
async fn drain_direct_dispatch(
    pool: &PgPool,
    state: &AppState,
    cloud: &pidash_db::config::CloudAgentSettings,
    cache: &RedisAdmissionCache,
    dispatches: Vec<Uuid>,
    deferred: Vec<DeferredConsume>,
    now: DateTime<Utc>,
) -> Result<(), Response> {
    for consume in &deferred {
        consume_admission_token(cache, consume);
    }
    for run_id in dispatches {
        match dispatch_agent_run(pool, cloud, run_id, now).await {
            Ok(DispatchDecision::PodDrain { pod_id }) => {
                if crate::runner_enroll::teardown::drain_pod_by_id(pool, state, pod_id)
                    .await
                    .is_err()
                {
                    return Err(server_error());
                }
            }
            Ok(_) => {}
            Err(_) => return Err(server_error()),
        }
    }
    Ok(())
}

/// Re-read the created run and render the 201: `run.refresh_from_db()`
/// then `AgentRunSerializer(run).data` — the direct path only
/// (`runs.py:376-380`). The issue-bound tails must NOT use this: Django
/// serializes the pre-dispatch in-memory instance there (no refresh), so
/// they snapshot the row in-transaction and render via
/// [`render_created_snapshot`].
async fn render_created_run(pool: &PgPool, run_id: Uuid) -> Response {
    let guarded = match fetch_guarded_run(pool, run_id).await {
        Ok(Some(guarded)) => guarded,
        Ok(None) => return server_error(),
        Err(response) => return response,
    };
    render_created_snapshot(pool, &guarded.run).await
}

/// Render the 201 from a pre-commit snapshot of the created run (the
/// issue-bound tails): Django's `AgentRunSerializer(run).data` on the
/// in-memory instance. The run's own columns come from the snapshot;
/// pod, tool calls and binding re-read post-commit — Django lazy-loads
/// those related rows at serialization time too, and the dispatch
/// touches only the `agent_run` row.
async fn render_created_snapshot(pool: &PgPool, run: &AgentRun) -> Response {
    let pod = match pod_mini_for(pool, run.pod_id).await {
        Ok(pod) => pod,
        Err(response) => return response,
    };
    let tool_calls = match tool_calls_for(pool, run.id).await {
        Ok(calls) => calls,
        Err(response) => return response,
    };
    let binding = match binding_detail_for(pool, run).await {
        Ok(binding) => binding,
        Err(response) => return response,
    };
    let payload = match render_detail(pool, run, &pod, &tool_calls, binding.as_ref(), None).await {
        Ok(payload) => payload,
        Err(response) => return response,
    };
    json_response(StatusCode::CREATED, payload.to_string())
}

/// Read the just-created run through the open creation transaction: the
/// pre-dispatch row Django's in-memory instance holds. A missing row or
/// a read error is a 500 — the caller rolls the transaction back, as
/// Django's `atomic` block would on the same failure.
async fn fetch_created_run(tx: &mut DbTransaction<'_>, run_id: Uuid) -> Result<AgentRun, Response> {
    let row = orm::agent_run::Entity::find_by_id(run_id)
        .one(&TxConn::new(tx.inner()))
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Err(server_error());
    };
    run_from_model(row).map_err(|_| server_error())
}

/// The direct-create reads `execution_fields` needs beyond the §6.5
/// context: the execution-scope project, the creator flags + LLM-key
/// presence, and the per-issue executor override.
struct DirectScope {
    project_id: Uuid,
    project_workspace_id: Uuid,
    default_agent_executor: String,
    actor_flags: UserFlags,
    has_llm_key: bool,
    requested: Option<String>,
}

/// Load the [`DirectScope`]: project (must exist — the pod FK says so),
/// creator flags (must exist — the session says so), creator key
/// presence (missing reads keyless), and the `requested` override
/// (missing/soft-deleted reads `None`, like `.first()`).
async fn load_direct_scope(
    pool: &PgPool,
    ctx: &ValidatedRunContext,
) -> Result<DirectScope, Response> {
    let db = sea_orm_connection(pool);
    let project = direct_project_select(ctx.pod.project_id)
        .one(&db)
        .await
        .map_err(|_| server_error())?;
    let Some((project_id, project_workspace_id, default_agent_executor)) = project else {
        return Err(server_error());
    };
    let flags = direct_actor_select(ctx.created_by)
        .one(&db)
        .await
        .map_err(|_| server_error())?;
    let Some((is_active, is_bot)) = flags else {
        return Err(server_error());
    };
    let key = direct_llm_config_select(ctx.created_by)
        .one(&db)
        .await
        .map_err(|_| server_error())?;
    let has_llm_key = key.flatten().is_some_and(|key| !key.is_empty());
    let requested: Option<Option<String>> = match ctx.work_item_id {
        Some(work_item_id) => direct_requested_select(work_item_id)
            .one(&db)
            .await
            .map_err(|_| server_error())?,
        None => None,
    };
    Ok(DirectScope {
        project_id,
        project_workspace_id,
        default_agent_executor,
        actor_flags: UserFlags { is_active, is_bot },
        has_llm_key,
        requested: requested.flatten(),
    })
}

/// Python `repr()` of one float (CPython `float_repr_style short`,
/// `PyOS_double_to_string` format `'r'`): shortest round-trip digits,
/// fixed notation for `-4 < e < 16`, else `d[.ddd]e±XX` with a signed
/// at-least-two-digit exponent; integral fixed values keep `.0`.
/// NaN/inf spell as Python does (unreachable from JSON parsing, which
/// rejects them — belt and braces).
fn py_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "inf".to_owned()
        } else {
            "-inf".to_owned()
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0".to_owned()
        } else {
            "0.0".to_owned()
        };
    }
    // Ryū digits (`serde_json`, like every other `py_float_repr` twin):
    // Rust `{:?}` (Grisu) picks a different last digit than CPython on
    // rare shortest-ties; Ryū matches `repr()` digit-for-digit
    // (differential-tested, review PIDASHCONV-1041). Reformat its
    // `1e16`/`1e-5` shapes into Python's `1e+16`/`1e-05` +
    // fixed-notation rules.
    let debug = serde_json::to_string(&value).expect("finite float formats");
    let (mantissa, exp10) = match debug.split_once(['e', 'E']) {
        Some((mantissa, exp)) => (mantissa, exp.parse::<i32>().unwrap_or(0)),
        None => (debug.as_str(), 0),
    };
    let negative = mantissa.starts_with('-');
    let mantissa = mantissa.strip_prefix('-').unwrap_or(mantissa);
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((int_part, frac_part)) => (int_part, frac_part),
        None => (mantissa, ""),
    };
    let mut digits = format!("{int_part}{frac_part}");
    while digits.len() > 1 && digits.starts_with('0') {
        digits.remove(0);
    }
    // Trailing zeros stay: they belong to the value (`100.0` is `D=1000`,
    // not `D=1`). With `D` the digit integer and `k` its scale, the
    // one-digit-before-point exponent is `e = k + n - 1`.
    let scale = exp10 - frac_part.len() as i32;
    let sci_exp = scale + digits.len() as i32 - 1;
    if !(-4..16).contains(&sci_exp) {
        let head = digits.chars().next().unwrap_or('0');
        let mut tail: String = digits.chars().skip(1).collect();
        while tail.ends_with('0') {
            tail.pop();
        }
        let sign = if sci_exp < 0 { '-' } else { '+' };
        let mag = sci_exp.abs();
        return format!(
            "{}{head}{}e{sign}{mag:02}",
            if negative { "-" } else { "" },
            if tail.is_empty() {
                String::new()
            } else {
                format!(".{tail}")
            },
        );
    }
    // Fixed: the point sits `e + 1` digits into the digit string
    // (`0.0001`: -3 → `0.0001`; `100.0`: 3 → `100.0`).
    let point = sci_exp + 1;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if point <= 0 {
        out.push('0');
        out.push('.');
        out.extend(std::iter::repeat_n('0', (-point) as usize));
        out.push_str(&digits);
    } else {
        let point = point as usize;
        if point >= digits.len() {
            out.push_str(&digits);
            out.extend(std::iter::repeat_n('0', point - digits.len()));
            out.push_str(".0");
        } else {
            out.push_str(&digits[..point]);
            out.push('.');
            out.push_str(&digits[point..]);
        }
    }
    out
}

/// Python `repr()` of one string: single quotes unless the text holds
/// one (then double, unless it holds both — then single with `\'`),
/// with `\n`/`\t`/`\r`/`\\` escapes and `\x`/`\u`/`\U` for the rest
/// of the non-printables. Printable non-ASCII passes through.
fn py_repr_str(text: &str) -> String {
    let mut out = String::from("'");
    let use_double = text.contains('\'') && !text.contains('"');
    if use_double {
        out = String::from("\"");
    }
    for c in text.chars() {
        match c {
            '\'' if !use_double => out.push_str("\\'"),
            '"' if use_double => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let code = c as u32;
                if code < 0x100 {
                    out.push_str(&format!("\\x{code:02x}"));
                } else if code < 0x10000 {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(if use_double { '"' } else { '\'' });
    out
}

/// Python `str()` of one JSON request value: what the `TextField`
/// `get_prep_value` stores for a non-string prompt (`runs.py:308`) and
/// what the cloud `f"{raw_prompt}"` interpolation renders
/// (`composer.py:512`). Strings verbatim; integers decimal (arbitrary
/// precision text is already exact); floats via [`py_float_repr`]
/// (JSON `1e3` parses to `1000.0`, whose `str` is `"1000.0"`, not the
/// raw text); containers via `repr` (`, ` separators, insertion order —
/// `preserve_order` is on).
fn py_str_json(raw: &Value) -> String {
    match raw {
        Value::String(text) => text.clone(),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                // Display the value, not the literal: `-0` is `0`.
                int.to_string()
            } else if let Some(uint) = number.as_u64() {
                uint.to_string()
            } else {
                let text = number.to_string();
                // Integers past `u64` keep their exact text
                // (`arbitrary_precision` is on); `str()` of a Python int
                // is exact, so never route them through `f64` (review,
                // PIDASHCONV-1041).
                let digits = text.strip_prefix('-').unwrap_or(&text);
                if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                    text
                } else {
                    match number.as_f64() {
                        Some(float) => py_float_repr(float),
                        None => text,
                    }
                }
            }
        }
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Null => "None".to_owned(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr_json).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, value)| format!("{}: {}", py_repr_str(key), py_repr_json(value)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr()` of one JSON request value (containers nest it).
fn py_repr_json(raw: &Value) -> String {
    match raw {
        Value::String(text) => py_repr_str(text),
        _ => py_str_json(raw),
    }
}

/// Render the cloud prompt for an issue-bound direct run
/// (`composer.py:459-514` with `issue is not None`): the full
/// `build_context` over the seam-loaded bundle, composed as
/// `KIND_DIRECT` with the user task wrapped as inert data. Returns the
/// prompt text plus the v2 manifest stamp. Runs inside the insertion
/// transaction (the bundle's prior-run count sees the new row, exactly
/// like Django's in-`atomic` read).
#[allow(clippy::too_many_arguments)]
async fn render_cloud_issue_prompt<'a>(
    state: &'a AppState,
    pool: &'a PgPool,
    tx: DbTransaction<'a>,
    issue_id: Uuid,
    run_id: Uuid,
    actor_id: Uuid,
    raw_prompt: &'a str,
    tool_plan: &'a Value,
    now_unix_secs: i64,
) -> Result<(DbTransaction<'a>, String, Value), Response> {
    use pidash_services::orchestration::creation::RenderBundle;
    use pidash_services::prompting::recipes;

    let row = dispatch_issue_select(issue_id)
        .one(&sea_orm_connection(pool))
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        let _ = tx.rollback().await;
        return Err(server_error());
    };
    let issue_row = row.into_issue_row();
    let Some(project_id) = issue_row.project_id else {
        let _ = tx.rollback().await;
        return Err(server_error());
    };
    let project_row = match fetch_project(pool, &project_id).await {
        Ok(Some(project_row)) => project_row,
        Ok(None) => {
            let _ = tx.rollback().await;
            return Err(server_error());
        }
        Err(_) => {
            let _ = tx.rollback().await;
            return Err(server_error());
        }
    };
    let (deps, _matcher) = match dispatch_deps(
        state,
        pool,
        actor_id,
        &issue_row,
        &project_row,
        now_unix_secs,
    )
    .await
    {
        Ok(deps) => deps,
        Err(_) => {
            let _ = tx.rollback().await;
            return Err(server_error());
        }
    };
    let mut store = LiveCreationStore::new(tx, pool, deps);
    let bundle: RenderBundle = match CreationSeam::render_bundle(
        &mut store,
        issue_id,
        run_id,
        None,
        TRIGGER_DIRECT,
        actor_id,
    )
    .await
    {
        Ok(bundle) => bundle,
        Err(_) => {
            let (tx, _, _, _, _, _) = store.into_parts();
            let _ = tx.rollback().await;
            return Err(server_error());
        }
    };
    // The `render_first_turn` assembly with `KIND_DIRECT`: same context
    // keys Django's `build_context(issue, run)` produces for the direct
    // row (`trigger="direct"`, `phase_kind=""`).
    let issue = &bundle.issue;
    let project = &bundle.project;
    let related = prompting_context::related_context(
        &issue.id.to_string(),
        &bundle.relation_rows,
        &bundle.refs,
    );
    let by_type = prompting_context::directional_relations_context(
        &issue.id.to_string(),
        &bundle.relation_rows,
        &bundle.directional_refs,
    );
    let relations = prompting_context::relations_context(&by_type);
    let repo = prompting_context::repo_context(
        &prompting_context::ProjectRepoView {
            url: project.repo_url.clone(),
            base_branch: project.base_branch.clone(),
        },
        issue.git_work_branch.as_deref(),
        bundle.remote.as_ref(),
        bundle.adapter.as_ref(),
    );
    let extra_toolsets_enabled = tool_plan
        .get("extra_toolsets")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let schema_tool = if extra_toolsets_enabled {
        state.toolsets().schema_tool_name().to_owned()
    } else {
        String::new()
    };
    let input = prompting_context::IssueContextInput {
        issue_id: issue.id.to_string(),
        project_identifier: project.identifier.clone(),
        sequence_id: i64::from(issue.sequence_id),
        title: issue.name.clone(),
        description_stripped: issue.description_stripped.clone(),
        state_name: bundle.state.as_ref().map(|state| state.name.clone()),
        state_group: bundle.state.as_ref().map(|state| state.group.clone()),
        priority: issue.priority.clone(),
        labels: bundle.labels.clone(),
        assignees: bundle.assignees.clone(),
        target_date_iso: issue
            .target_date
            .map(|date| date.format("%Y-%m-%d").to_string()),
        project_states: bundle.project_states.clone(),
        workspace_slug: bundle.workspace_slug.clone(),
        workspace_name: bundle.workspace_name.clone(),
        project_id: project.id.to_string(),
        project_name: project.name.clone(),
        project_description: project.description.clone(),
        repo,
        code_reviews: prompting_context::code_reviews_context(&bundle.reviews),
        parent: bundle.parent.clone(),
        ancestors: bundle.lineage.clone(),
        children: Value::Array(prompting_context::children_context(&bundle.children)),
        related: Value::Array(related),
        relations,
        run_id: run_id.to_string(),
        run_template_name: recipes::KIND_DIRECT.to_owned(),
        attempt: prompting_context::compute_attempt(bundle.prior_run_count),
        trigger: Some(TRIGGER_DIRECT.to_owned()),
        executor_kind: AgentExecutorKind::CloudAgent.value().to_owned(),
        available_tools: tool_plan.get("tools").cloned().unwrap_or(Value::Null),
        unavailable_capabilities: tool_plan
            .get("unavailable_capabilities")
            .cloned()
            .unwrap_or(Value::Null),
        extra_toolsets_enabled,
        extra_toolsets_schema_tool: schema_tool,
        limits: tool_plan.get("limits").cloned().unwrap_or(Value::Null),
        tick: tick_value(
            bundle.ticker.as_ref(),
            project.pool,
            project.interval_impl,
            project.interval_review,
            project.interval_test,
            bundle.state.as_ref(),
        ),
        comments_section: prompting_context::comments_section(&bundle.comments),
        parent_done_payload: parent_done_payload(
            bundle.direct_parent_payload.as_ref(),
            bundle.has_parent_run,
            bundle.ticker_parent_payload.as_ref(),
        ),
        workpad_body: issue.workpad.clone().unwrap_or_default(),
    };
    let context_value = prompting_context::build_context(&input);
    let catalog_version = tool_plan
        .get("catalog_version")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let turn = match prompting_context::build_direct_turn(
        raw_prompt,
        Some(&context_value),
        Some(AgentExecutorKind::CloudAgent.value()),
        catalog_version,
    ) {
        Ok(turn) => turn,
        Err(_) => {
            let (tx, _, _, _, _, _) = store.into_parts();
            let _ = tx.rollback().await;
            return Err(server_error());
        }
    };
    let (tx, _, _, _, _, _) = store.into_parts();
    let manifest = turn
        .manifest
        .as_ref()
        .map(|manifest| manifest.to_json())
        .unwrap_or(Value::Null);
    Ok((tx, turn.text, manifest))
}

/// Render the cloud prompt for a free-form direct run
/// (`composer.py:475-498`): the small inline context (workspace slug +
/// name, project id/identifier/name/description, the `direct` run
/// block, tools, limits), composed as `KIND_DIRECT`.
async fn render_cloud_free_prompt(
    state: &AppState,
    pool: &PgPool,
    run_id: Uuid,
    workspace_id: Uuid,
    project_id: Uuid,
    raw_prompt: &str,
    tool_plan: &Value,
) -> Result<(String, Value), Response> {
    let db = sea_orm_connection(pool);
    let workspace = direct_workspace_select(workspace_id)
        .one(&db)
        .await
        .map_err(|_| server_error())?;
    let Some((workspace_slug, workspace_name)) = workspace else {
        return Err(server_error());
    };
    let project = direct_context_project_select(project_id)
        .one(&db)
        .await
        .map_err(|_| server_error())?;
    let Some((project_id, project_identifier, project_name, project_description)) = project else {
        return Err(server_error());
    };
    let extra_toolsets_enabled = tool_plan
        .get("extra_toolsets")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // `extra_toolsets_vars(run)` (`context.py:514-517`): consulted only
    // when the plan enables them; CE names none.
    let schema_tool = if extra_toolsets_enabled {
        state.toolsets().schema_tool_name().to_owned()
    } else {
        String::new()
    };
    let input = prompting_context::DirectContextInput {
        run_id: run_id.to_string(),
        workspace_slug,
        workspace_name,
        project_id: project_id.to_string(),
        project_identifier,
        project_name,
        project_description,
        trigger: Some(TRIGGER_DIRECT.to_owned()),
        available_tools: tool_plan.get("tools").cloned().unwrap_or(Value::Null),
        unavailable_capabilities: tool_plan
            .get("unavailable_capabilities")
            .cloned()
            .unwrap_or(Value::Null),
        extra_toolsets_enabled,
        extra_toolsets_schema_tool: schema_tool,
        limits: tool_plan.get("limits").cloned().unwrap_or(Value::Null),
    };
    let context_value = prompting_context::direct_context(&input);
    let catalog_version = tool_plan
        .get("catalog_version")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let turn = match prompting_context::build_direct_turn(
        raw_prompt,
        Some(&context_value),
        Some(AgentExecutorKind::CloudAgent.value()),
        catalog_version,
    ) {
        Ok(turn) => turn,
        Err(_) => return Err(server_error()),
    };
    let manifest = turn
        .manifest
        .as_ref()
        .map(|manifest| manifest.to_json())
        .unwrap_or(Value::Null);
    Ok((turn.text, manifest))
}

/// The direct-create llm gate through the installed assistant seam
/// (`user_has_llm_config`, `agent_execution.py:69-80`): the CE default
/// answers the BYOK key bit; the overlay routes OpenHub/Codex/BYOK
/// through the same single switch point Pi Dash AI uses (PIDASHCONV-1051).
async fn direct_gate_allows(state: &AppState, actor_id: Uuid, has_key: bool) -> bool {
    state
        .assistant_seam()
        .has_usable_llm_config(actor_id, has_key)
        .await
}

/// Direct create (`runs.py:238-345`): the falsy-prompt 400 is answered
/// by the caller; everything from `validate_run_creation` down runs
/// here. Parse (falsy/None/coerce per the Django `filter()` rules,
/// probed), §6.5 validation with the raw-text comparison post-check,
/// then the `execution_fields` + insert + prompt + cap transaction,
/// post-commit dispatch, and the 201.
/// The direct tail's pin decision (`runs.py:258-284`): the `pod_once`
/// gate, the H-11 refusal, then the pin resolver. Returns the
/// `(pinned_runner_id, pin_kind)` the insert binds, or the refusal
/// `Response` to answer with.
async fn resolve_direct_pin(
    pool: &PgPool,
    actor_id: Uuid,
    work_item_id: Option<Uuid>,
    pod_once: bool,
    pod_id: Uuid,
    pod_name: &str,
) -> Result<(Option<Uuid>, &'static str), Response> {
    let orm_pool = pidash_db::pool::sea_orm_connection(pool);
    match work_item_id {
        Some(work_item_id) => {
            let Some(issue) = pidash_db::entity::issues::Entity::find_by_id(work_item_id)
                .one(&orm_pool)
                .await
                .map_err(|_| server_error())?
            else {
                return Err(server_error());
            };
            if pod_once {
                // The one-shot pod path (`:264-274`): decided by the
                // view (this tail), before reconcile. Auth first, the
                // pod gate second; `no_eligible_runner_in_pod` is a
                // preflight outcome, not an auth code.
                if let Some(code) = pidash_services::domain::issue_assignment::pod_once_auth_code(
                    &orm_pool,
                    &issue,
                    Some(actor_id),
                )
                .await
                .map_err(|_| server_error())?
                {
                    let detail = match code {
                        "pod_once_not_applicable" => {
                            "pod_once requires a dedicated runner on the issue"
                        }
                        _ => "only the issue's creator and assignees may use pod_once",
                    };
                    let status = if code == "pod_once_not_applicable" {
                        StatusCode::BAD_REQUEST
                    } else {
                        StatusCode::FORBIDDEN
                    };
                    return Err(json_response(
                        status,
                        serde_json::to_string(&serde_json::json!({"error": detail, "code": code}))
                            .expect("serializable refusal"),
                    ));
                }
                let pod_has = pidash_services::runner_sessions::pod_match::pod_has_runner_for_issue_principal(
                    &orm_pool,
                    pod_id,
                    work_item_id,
                    Some(actor_id),
                    true,
                    true,
                )
                .await
                .map_err(|_| server_error())?;
                if !pod_has {
                    return Err(json_response(
                        StatusCode::CONFLICT,
                        serde_json::to_string(&serde_json::json!({"error": format!("no eligible runner in pod {}", pod_name), "code": "no_eligible_runner_in_pod"}))
                            .expect("serializable refusal"),
                    ));
                }
                Ok((
                    None,
                    pidash_services::domain::issue_assignment::PIN_LOCALITY,
                ))
            } else {
                // H-11 (§11.2): a person's run on an offline/paused
                // dedicated runner is refused at creation, before
                // reconcile, with the full §11.2 body.
                if let Some(code) =
                    pidash_services::domain::issue_assignment::dedicated_run_refusal(
                        &orm_pool,
                        &issue,
                        true,
                        Some(actor_id),
                        false,
                    )
                    .await
                    .map_err(|_| server_error())?
                {
                    let refusal =
                        pidash_services::domain::issue_assignment::dedicated_refusal_body(
                            &orm_pool, &issue, code,
                        )
                        .await
                        .map_err(|_| server_error())?;
                    return Err(json_response(StatusCode::CONFLICT, refusal.to_string()));
                }
                // For a dedicated issue the pin is
                // `issue.assigned_runner` (§9.1); otherwise the
                // locality pin — with `fresh_session` or no parent
                // the locality pin is `None`, matching Django
                // (`runs.py:281-284` passes `parent=None` here).
                let resolved = pidash_services::domain::issue_assignment::resolve_run_pin(
                    &orm_pool,
                    &pidash_services::domain::issue_assignment::ResolvePinArgs {
                        issue: &issue,
                        parent: None,
                        pod_id,
                        fresh_session: false,
                        creator_id: Some(actor_id),
                        trigger: pidash_types::runner_runs::AgentRunTrigger::Direct,
                        pod_once: false,
                        actor_id,
                    },
                )
                .await
                .map_err(|_| server_error())?;
                let pin_id = resolved.runner.as_ref().map(|r| r.id);
                let kind = resolved.pin_kind;
                Ok((pin_id, kind))
            }
        }
        None => Ok((
            None,
            pidash_services::domain::issue_assignment::PIN_LOCALITY,
        )),
    }
}

pub async fn post_direct(
    state: &AppState,
    pool: &PgPool,
    actor_id: Uuid,
    body: &Value,
) -> Response {
    // `request.data.get("workspace")`: falsy → 400 (`:67-68`), garbage
    // → 500 (the membership `filter()` raises), else the id.
    let raw_workspace = body.get("workspace");
    if !py_truthy_json(raw_workspace) {
        return run_creation_denial(&RunCreationError::WorkspaceRequired);
    }
    let (workspace_id, workspace_raw) = match coerce_filter_uuid(raw_workspace) {
        UuidCoerce::Id { id, raw } => (id, raw),
        // Absent is unreachable past the falsy check; Invalid is the
        // `ValidationError` Django answers 500.
        UuidCoerce::Absent | UuidCoerce::Invalid => return server_error(),
    };
    // `request.data.get("work_item")`: `None` skips (`is not None`,
    // `:79`), garbage → 500.
    let work_item_id = match coerce_filter_uuid(body.get("work_item")) {
        UuidCoerce::Absent => None,
        UuidCoerce::Id { id, .. } => Some(id),
        UuidCoerce::Invalid => return server_error(),
    };
    // `request.data.get("pod")`: same shape (`:130`).
    let pod_id = match coerce_filter_uuid(body.get("pod")) {
        UuidCoerce::Absent => None,
        UuidCoerce::Id { id, .. } => Some(id),
        UuidCoerce::Invalid => return server_error(),
    };

    let store = PoolRunCreationStore::new(pool);
    let ctx = match validation::validate_run_creation(
        &store,
        Some(actor_id),
        Some(workspace_id),
        work_item_id,
        pod_id,
    )
    .await
    {
        Ok(ctx) => ctx,
        Err(ValidationError::Invalid(error))
            if workspace_raw != workspace_id.to_string() && work_item_id.is_some() =>
        {
            // Django's work-item `str() != str()` check
            // (`validation.py:87-92`) fires before pod resolution, so with
            // a work item every pod error under a non-canonical workspace
            // reads as the work-item mismatch (review, PIDASHCONV-1041).
            return run_creation_denial(&match error {
                RunCreationError::NoPodAvailable
                | RunCreationError::PodProjectMismatch
                | RunCreationError::PodMissing
                | RunCreationError::PodWorkspaceMismatch => {
                    RunCreationError::WorkItemWorkspaceMismatch
                }
                other => other,
            });
        }
        Err(ValidationError::Invalid(error)) => return run_creation_denial(&error),
        Err(ValidationError::Store(_)) => return server_error(),
    };
    // Raw-text post-check: a non-canonical workspace rendering (upper /
    // braced / int / `True`) passes the membership `filter()` (which
    // coerces) but fails every `str() != str()` comparison it reaches
    // (probed). The typed validation compares canonical `Uuid`s, so
    // re-apply the raw comparison here, in Django's check order.
    if workspace_raw != workspace_id.to_string() {
        if work_item_id.is_some() {
            return run_creation_denial(&RunCreationError::WorkItemWorkspaceMismatch);
        }
        if pod_id.is_some() {
            return run_creation_denial(&RunCreationError::PodWorkspaceMismatch);
        }
    }

    let scope = match load_direct_scope(pool, &ctx).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    // The pin (design §9.1) + the H-11 refusal (§11) + `pod_once`
    // (§23.3), all before the insert exactly as Django's direct tail
    // (`runs.py:258-284`): the H-11 refusal is checked before reconcile
    // and fires without creating, the pod_once gate before that for
    // the one-shot path, the resolver last for this tail.
    let pod_once = body.get("pin").and_then(Value::as_str).unwrap_or("").trim() == "pod_once";
    let (pinned_runner_id, pin_kind) = match resolve_direct_pin(
        pool,
        actor_id,
        ctx.work_item_id,
        pod_once,
        ctx.pod.id,
        &ctx.pod.name,
    )
    .await
    {
        Ok(pin) => pin,
        Err(response) => return response,
    };
    // `request.data.get("run_config") or {}` /
    // `request.data.get("required_capabilities") or []` (`:287-288`):
    // stored verbatim, falsy → the default.
    let run_config = match body.get("run_config") {
        Some(config) if py_truthy_json(Some(config)) => config.clone(),
        _ => Value::Object(serde_json::Map::new()),
    };
    let required_raw = match body.get("required_capabilities") {
        Some(caps) if py_truthy_json(Some(caps)) => caps.clone(),
        _ => Value::Array(Vec::new()),
    };
    let prompt_raw = body.get("prompt").cloned().unwrap_or(Value::Null);
    let now = now_micros();

    let mut tx = match DbTransaction::begin(pool).await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let project = ProjectScope {
        project_id: scope.project_id,
        workspace_id: scope.project_workspace_id,
        default_agent_executor: scope.default_agent_executor.clone(),
    };
    let actor = ActorScope {
        id: actor_id,
        flags: scope.actor_flags,
    };
    let cloud = state.settings().cloud_agent.clone();
    let managed = state.settings().managed_runner.clone();
    // Only the cloud planner inspects capabilities: pre-resolve the
    // executor and convert strictly on the cloud path (a `TypeError`
    // there is Django's 500); anywhere else the raw value is stored
    // and never looked at.
    let pre_resolved = resolve_executor_kind(
        scope.requested.as_deref(),
        &project.default_agent_executor,
        &cloud,
    )
    .ok();
    let capabilities: Vec<String> = if pre_resolved == Some(AgentExecutorKind::CloudAgent) {
        match convert_capabilities(&required_raw) {
            Ok(capabilities) => capabilities,
            Err(()) => return rollback_then(tx, server_error()).await,
        }
    } else {
        Vec::new()
    };
    let capability_refs: Vec<&str> = capabilities.iter().map(String::as_str).collect();
    // Only the cloud arm consults the llm gate (`creation.py:33-39`):
    // like capabilities above, consult the installed seam only when
    // the pre-resolve lands on the cloud path, so local and managed
    // creates never pay for the overlay's reads (and a resolution
    // failure still raises in the driver before any gate verdict).
    let has_llm = if pre_resolved == Some(AgentExecutorKind::CloudAgent) {
        direct_gate_allows(state, actor_id, scope.has_llm_key).await
    } else {
        false
    };
    let inputs = ExecutionInputs {
        project: &project,
        run_kind: RUN_KIND_DIRECT,
        has_issue: ctx.work_item_id.is_some(),
        required_capabilities: &capability_refs,
        actor: Some(&actor),
        automatic: false,
        requested: scope.requested.as_deref(),
        now_unix_secs: now.timestamp(),
    };
    let cache = RedisAdmissionCache {
        client: client_from_settings(state.settings()),
    };
    let mut deferred: Vec<DeferredConsume> = Vec::new();
    let mut collect = |consume: DeferredConsume| deferred.push(consume);
    let fields = match resolve_execution_fields(
        &mut tx,
        &inputs,
        &cloud,
        &managed,
        &cache,
        &mut collect,
        ExecutionSeams {
            has_usable_llm_config: move || has_llm,
            toolsets: state.toolsets().clone(),
        },
    )
    .await
    {
        Ok(fields) => fields,
        Err(error) => {
            let denial = execution_fields_denial(&error);
            return rollback_then(tx, denial).await;
        }
    };
    let run_id = Uuid::new_v4();
    let executor_kind = fields.executor_kind.value().to_owned();
    // The managed waiting path stamps its code into the bag at creation
    // (`cloud_agent/creation.py`, `fields["error_details"]`).
    let error_details = pidash_services::orchestration::initial_error_details(
        fields.error_code.as_deref().unwrap_or_default(),
    );
    let insert = orm::agent_run::Entity::insert(
        DirectRunInsert {
            run_id,
            workspace_id: ctx.workspace_id,
            created_by_id: ctx.created_by,
            pod_id: ctx.pod.id,
            // The resolved pin (dedicated or locality, §9.2) is what
            // Django's `AgentRun.objects.create(pinned_runner=...)`
            // passes; the execution-fields pick (managed) only fills an
            // unpinned run.
            pinned_runner_id: pinned_runner_id.or(fields.pinned_runner_id),
            pin_kind,
            work_item_id: ctx.work_item_id,
            executor_kind: executor_kind.as_str(),
            error_details: &error_details,
            tool_plan: &fields.tool_plan,
            run_config: &run_config,
            required_capabilities: &required_raw,
            created_at: now,
        }
        .active_model(),
    )
    .exec(&TxConn::new(tx.inner()))
    .await;
    if let Err(error) = insert {
        // `except IntegrityError` → 409, whatever the constraint.
        let denial = if pidash_db::orm_tx::is_integrity_violation(&error) {
            json_response(StatusCode::CONFLICT, ACTIVE_RUN_EXISTS_BODY.to_owned())
        } else {
            server_error()
        };
        return rollback_then(tx, denial).await;
    }
    // `build_direct_turn` (`:293-299`): local and managed runs keep the
    // raw prompt verbatim with a null manifest; cloud runs compose.
    let prompt_text = py_str_json(&prompt_raw);
    let (mut tx, prompt, manifest) = if fields.executor_kind == AgentExecutorKind::CloudAgent {
        let outcome = match ctx.work_item_id {
            Some(work_item_id) => {
                render_cloud_issue_prompt(
                    state,
                    pool,
                    tx,
                    work_item_id,
                    run_id,
                    actor_id,
                    &prompt_text,
                    &fields.tool_plan,
                    now.timestamp(),
                )
                .await
            }
            None => match render_cloud_free_prompt(
                state,
                pool,
                run_id,
                ctx.workspace_id,
                scope.project_id,
                &prompt_text,
                &fields.tool_plan,
            )
            .await
            {
                Ok((prompt, manifest)) => Ok((tx, prompt, manifest)),
                Err(response) => return rollback_then(tx, response).await,
            },
        };
        match outcome {
            Ok(outcome) => outcome,
            Err(response) => return response,
        }
    } else {
        (tx, prompt_text, Value::Null)
    };
    // The prompt-size cap is a Cloud Agent execution limit
    // (`:300-308`); local runs stream the prompt verbatim.
    if fields.executor_kind == AgentExecutorKind::CloudAgent
        && prompt.len() > state.settings().cloud_agent.max_prompt_bytes.max(0) as usize
    {
        return rollback_then(
            tx,
            json_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                PROMPT_TOO_LARGE_BODY.to_owned(),
            ),
        )
        .await;
    }
    let manifest_bind = if manifest.is_null() {
        None
    } else {
        Some(&manifest)
    };
    if let Err(error) = prompt_update_statement(run_id, &prompt, manifest_bind)
        .exec(&TxConn::new(tx.inner()))
        .await
    {
        let denial = if pidash_db::orm_tx::is_integrity_violation(&error) {
            json_response(StatusCode::CONFLICT, ACTIVE_RUN_EXISTS_BODY.to_owned())
        } else {
            server_error()
        };
        return rollback_then(tx, denial).await;
    }
    let mut dispatches: Vec<Uuid> = Vec::new();
    collect_dispatch(&mut |id| dispatches.push(id), run_id);
    if tx.commit().await.is_err() {
        return server_error();
    }
    if let Err(response) =
        drain_direct_dispatch(pool, state, &cloud, &cache, dispatches, deferred, now).await
    {
        return response;
    }
    // The one-shot pod path records its system comment on commit
    // (`runs.py:417-420` calls `_post_pod_once_started_comment`).
    if pod_once {
        if let Some(work_item_id) = ctx.work_item_id {
            let orm = pidash_db::pool::sea_orm_connection(pool);
            if pidash_services::domain::issue_assignment::post_pod_once_started_comment(
                &orm,
                work_item_id,
                &ctx.pod.name,
                actor_id,
            )
            .await
            .is_err()
            {
                return server_error();
            }
        }
    }
    render_created_run(pool, run_id).await
}

// ---------------------------------------------------------------------------
// Issue-bound creates: `run_ai` + `comment_and_run` (`runs.py:347-466`)
// ---------------------------------------------------------------------------

/// Which issue-bound dispatch runs after the shared prefix + clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IssueDispatchMode {
    RunAi,
    CommentAndRun,
}

impl IssueDispatchMode {
    /// The `AgentRunTrigger` the clock entry and the created run carry.
    fn trigger(self) -> &'static str {
        match self {
            IssueDispatchMode::RunAi => TRIGGER_RUN_AI,
            IssueDispatchMode::CommentAndRun => TRIGGER_COMMENT_AND_RUN,
        }
    }

    /// The falsy-`work_item` 400 body (`runs.py:360-364/431-436`).
    fn missing_work_item_body(self) -> &'static str {
        match self {
            IssueDispatchMode::RunAi => super::runs::RUN_AI_WORK_ITEM_REQUIRED_BODY,
            IssueDispatchMode::CommentAndRun => super::runs::COMMENT_RUN_WORK_ITEM_REQUIRED_BODY,
        }
    }

    /// The dispatch-yielded-nothing 409 body (`runs.py:380-384/454-462`).
    fn dispatch_failed_body(self) -> &'static str {
        match self {
            IssueDispatchMode::RunAi => RUN_AI_DISPATCH_FAILED_BODY,
            IssueDispatchMode::CommentAndRun => COMMENT_RUN_DISPATCH_FAILED_BODY,
        }
    }
}

/// Fetch the v1 `IssueRow` for the dispatch tails ([`dispatch_issue_select`]).
async fn fetch_dispatch_issue(pool: &PgPool, issue_id: Uuid) -> Result<Option<IssueRow>, Response> {
    Ok(dispatch_issue_select(issue_id)
        .one(&sea_orm_connection(pool))
        .await
        .map_err(|_| server_error())?
        .map(DispatchIssueRow::into_issue_row))
}

/// Shared body of `_post_run_ai` and `_post_comment_and_run`: the
/// issue-bound prefix (falsy `work_item` 400, unknown issue / non-member
/// 404 over `all_objects`), then one transaction holding the human
/// clock (`reconcile` with `want_run=true` while the issue ticks — the
/// queued entry commits as the 202) and the D-12 dispatch, rolled back
/// when the dispatch yields no run (the re-time and any bounce writes
/// vanish with it, exactly like Django's `set_rollback(True)`).
async fn post_issue_dispatch(
    state: &AppState,
    pool: &PgPool,
    actor_id: Uuid,
    body: &Value,
    mode: IssueDispatchMode,
) -> Response {
    // `work_item = request.data.get("work_item"); if not work_item`
    // (`runs.py:359-364/431-436`).
    let raw_work_item = body.get("work_item");
    if !py_truthy_json(raw_work_item) {
        return json_response(
            StatusCode::BAD_REQUEST,
            mode.missing_work_item_body().to_owned(),
        );
    }
    // `Issue.all_objects.filter(pk=work_item_id).first()` (`:365/437`):
    // same `filter()` coercion as the direct path (ints coerce via
    // `UUID(int=)`, garbage → 500 — probed), no soft-delete scope.
    // `pod_once = (request.data.get("pin") or "").strip() == "pod_once"`
    // (`runs.py:483/577`). `pod_once` suppresses the queued 202
    // (`_human_run_clock`), fires the dedicated-refusal bodies
    // (`:492-532`), and bypasses the dedicated pin for this run
    // only (§23.3).
    let pod_once = body.get("pin").and_then(Value::as_str).unwrap_or("").trim() == "pod_once";
    let work_item_id = match coerce_filter_uuid(raw_work_item) {
        UuidCoerce::Id { id, .. } => id,
        UuidCoerce::Absent | UuidCoerce::Invalid => return server_error(),
    };
    let row = match issue_lookup(pool, work_item_id).await {
        Ok(row) => row,
        Err(response) => return response,
    };
    let Some((_, workspace_id)) = row else {
        return json_response(StatusCode::NOT_FOUND, ISSUE_NOT_FOUND_BODY.to_owned());
    };
    let role = match workspace_role(pool, workspace_id, actor_id).await {
        Ok(role) => role,
        Err(response) => return response,
    };
    if !is_workspace_member(role) {
        return json_response(StatusCode::NOT_FOUND, ISSUE_NOT_FOUND_BODY.to_owned());
    }

    let issue_row = match fetch_dispatch_issue(pool, work_item_id).await {
        Ok(Some(issue_row)) => issue_row,
        Ok(None) => return json_response(StatusCode::NOT_FOUND, ISSUE_NOT_FOUND_BODY.to_owned()),
        Err(response) => return response,
    };
    // `project_id` is `NOT NULL` and the project row exists (FK); a miss
    // is the `DoesNotExist` Django would raise — a 500.
    let project_id = match issue_row.project_id {
        Some(project_id) => project_id,
        None => return server_error(),
    };
    let project_row = match fetch_project(pool, &project_id).await {
        Ok(Some(project_row)) => project_row,
        Ok(None) => return server_error(),
        Err(_) => return server_error(),
    };
    let state_ref = match (
        issue_row.state_group.as_deref(),
        issue_row.state_name.as_deref(),
    ) {
        (Some(group), Some(name)) => Some(StateRef { group, name }),
        _ => None,
    };
    // `_human_run_clock` only runs while the issue ticks
    // (`agent_phases.is_ticking_state`, `runs.py:404`); otherwise the
    // clock is untouched and the dispatch runs bare.
    let ticking = is_ticking_state(state_ref.as_ref());
    let now = now_micros();
    let jitter = draw_jitter(&issue_row, &project_row.policy);
    let (deps, matcher) = match dispatch_deps(
        state,
        pool,
        actor_id,
        &issue_row,
        &project_row,
        now.timestamp(),
    )
    .await
    {
        Ok(deps) => deps,
        Err(_) => return server_error(),
    };
    let cloud = state.settings().cloud_agent.clone();

    let tx = match DbTransaction::begin(pool).await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let mut store = LiveCreationStore::new(tx, pool, deps);
    // Smallest failing unit first: the store comes apart for rollback.
    macro_rules! rollback_500 {
        () => {{
            let (tx, _, _, _, _, _) = store.into_parts();
            return rollback_then(tx, server_error()).await;
        }};
    }
    if ticking {
        // `scheduling.reconcile(issue, human_run_requested(actor, trigger))`
        // with the default `want_run=True` (`runs.py:406-408`): one free
        // run now, or a queued entry when a run is active.
        let mut ticker = match store.lock_ticker_for_issue(work_item_id).await {
            Ok(ticker) => ticker,
            Err(_) => rollback_500!(),
        };
        let has_active_run = match CreationSeam::active_run_for(&mut store, work_item_id).await {
            Ok(active) => active.is_some(),
            Err(_) => rollback_500!(),
        };
        let policy = match store.clock_policy_for_project(project_id).await {
            Ok(policy) => policy,
            Err(_) => rollback_500!(),
        };
        let clock_state = state_ref.as_ref().map(|state| StateRef {
            group: state.group,
            name: state.name,
        });
        let clock_issue = ClockIssue {
            issue_id: work_item_id,
            state: clock_state,
            policy,
        };
        let event = TickerEvent::human_run_requested(true, Some(actor_id), mode.trigger());
        let outcome = match clock::reconcile(
            &mut ticker,
            &clock_issue,
            &event,
            None,
            has_active_run,
            now,
            jitter,
            Some(actor_id),
        ) {
            Ok(outcome) => outcome,
            Err(_) => rollback_500!(),
        };
        match outcome.write {
            ClockWrite::None => {}
            ClockWrite::Insert => {
                let row = match ticker.as_ref() {
                    Some(row) => row,
                    None => rollback_500!(),
                };
                if store.insert_ticker(row).await.is_err() {
                    rollback_500!();
                }
            }
            ClockWrite::Update => {
                let row = match ticker.as_ref() {
                    Some(row) => row,
                    None => rollback_500!(),
                };
                if store.save_clock(row, now).await.is_err() {
                    rollback_500!();
                }
            }
        }
        if outcome.decision.queued && !pod_once {
            // The queued entry commits (`:373-374/409-417`): no dispatch.
            let (tx, _, _, _, _, _) = store.into_parts();
            if tx.commit().await.is_err() {
                return server_error();
            }
            let body = serde_json::to_value(&QueuedEntry {
                queued: true,
                reason: &outcome.decision.reason,
                detail: QUEUED_ENTRY_DETAIL,
            })
            .expect("serializable queue")
            .to_string();
            return json_response(StatusCode::ACCEPTED, body);
        }
    }
    // `dispatch_run_ai_run_with_reason` / `dispatch_continuation_run`
    // (`:375/447`).
    let (run_id, run_ai_reason) = match mode {
        IssueDispatchMode::RunAi => {
            match pidash_services::orchestration::dispatch::dispatch_run_ai_run_with_reason(
                &mut store,
                work_item_id,
                Some(actor_id),
                pod_once,
                &matcher,
                now,
                jitter,
            )
            .await
            {
                Ok(outcome) => (outcome.run_id, outcome.reason),
                Err(_) => rollback_500!(),
            }
        }
        IssueDispatchMode::CommentAndRun => {
            match pidash_services::orchestration::dispatch::dispatch_continuation_run(
                &mut store,
                work_item_id,
                mode.trigger(),
                Some(actor_id),
                pod_once,
                false,
                &matcher,
                now,
                jitter,
            )
            .await
            {
                Ok(outcome) => (outcome.run_id, None),
                Err(_) => rollback_500!(),
            }
        }
    };
    let (mut tx, pool, cache, dispatches, deferred, _terminal) = store.into_parts();
    let Some(run_id) = run_id else {
        // `transaction.set_rollback(True)` (`:379/453`): the re-time and
        // any bounce writes vanish with the failed dispatch. The
        // dedicated refusal / pod_once bodies are built first (`:492-520`).
        let response = refusal_response_for_status(
            pool,
            work_item_id,
            actor_id,
            pod_once,
            run_ai_reason.as_deref(),
            mode,
        )
        .await;
        let Some(response) = response else {
            return rollback_then(
                tx,
                json_response(StatusCode::CONFLICT, mode.dispatch_failed_body().to_owned()),
            )
            .await;
        };
        return rollback_then(tx, response).await;
    };
    // Snapshot the created row before commit and drain: Django answers
    // these tails with the pre-dispatch in-memory instance (no
    // `refresh_from_db`, unlike the direct path).
    let snapshot = match fetch_created_run(&mut tx, run_id).await {
        Ok(run) => run,
        Err(response) => return rollback_then(tx, response).await,
    };
    if tx.commit().await.is_err() {
        return server_error();
    }
    if let Err(response) =
        drain_direct_dispatch(pool, state, &cloud, &cache, dispatches, deferred, now).await
    {
        return response;
    }
    render_created_snapshot(pool, &snapshot).await
}

/// The dedicated / pod_once 409/403/400 body for a failed issue
/// dispatch (`runs.py:492-520` for run_ai, `:577-620` for
/// comment_and_run): the comment&run tail re-probes the refusal
/// (Django's `dispatch_continuation_run` returns None without a
/// The dedicated / pod_once 409/403/400 body for a failed issue
/// dispatch (`runs.py:492-520` for run_ai, `:577-620` for
/// comment_and_run): the comment&run tail re-probes the refusal
/// (Django's `dispatch_continuation_run` returns None without a
/// reason); the run_ai tail maps the dispatch's reason code.
async fn refusal_response_for_status(
    pool: &PgPool,
    work_item_id: Uuid,
    actor_id: Uuid,
    pod_once: bool,
    reason: Option<&str>,
    mode: IssueDispatchMode,
) -> Option<Response> {
    use pidash_db::entity::issues;

    // The comment&run tail re-probes first (`:577-620`); the run_ai
    // tail maps the dispatch's reason (`:492-520`).
    let code = match mode {
        IssueDispatchMode::RunAi => reason,
        IssueDispatchMode::CommentAndRun if !pod_once => {
            let orm_pool = pidash_db::pool::sea_orm_connection(pool);
            let issue = issues::Entity::find_by_id(work_item_id)
                .one(&orm_pool)
                .await
                .ok()
                .flatten()?;
            return match pidash_services::domain::issue_assignment::dedicated_run_refusal(
                &orm_pool,
                &issue,
                true,
                Some(actor_id),
                false,
            )
            .await
            .ok()
            .flatten()
            {
                Some(code) => Some(dedicated_code_response(pool, work_item_id, code).await),
                None => None,
            };
        }
        IssueDispatchMode::CommentAndRun => {
            // pod_once: the else maps to `no_eligible_runner_in_pod`
            // (`:603-607`).
            let orm_pool = pidash_db::pool::sea_orm_connection(pool);
            let issue = issues::Entity::find_by_id(work_item_id)
                .one(&orm_pool)
                .await
                .ok()
                .flatten()?;
            let code = pidash_services::domain::issue_assignment::pod_once_auth_code(
                &orm_pool,
                &issue,
                Some(actor_id),
            )
            .await
            .ok()
            .flatten()
            .unwrap_or("no_eligible_runner_in_pod");
            return Some(pod_once_code_response(code));
        }
    };
    // run_ai maps its dispatch's reason (`:492-520`).
    match mode {
        IssueDispatchMode::RunAi => {
            let code_owned = code.map(|code| code.to_owned())?;
            let code = code_owned;
            match code.as_str() {
                "assigned_runner_offline"
                | "assigned_runner_paused"
                | "assigned_runner_not_owner" => {
                    let orm_pool = pidash_db::pool::sea_orm_connection(pool);
                    let issue = issues::Entity::find_by_id(work_item_id)
                        .one(&orm_pool)
                        .await
                        .ok()
                        .flatten()?;
                    let code_s: &'static str = match code.as_str() {
                        "assigned_runner_offline" => "assigned_runner_offline",
                        "assigned_runner_paused" => "assigned_runner_paused",
                        _ => "assigned_runner_not_owner",
                    };
                    let body = pidash_services::domain::issue_assignment::dedicated_refusal_body(
                        &orm_pool, &issue, code_s,
                    )
                    .await
                    .ok()?;
                    Some(json_response(StatusCode::CONFLICT, body.to_string()))
                }
                "pod_once_not_allowed"
                | "pod_once_not_applicable"
                | "no_eligible_runner_in_pod" => Some(pod_once_code_response(code.as_str())),
                _ => None,
            }
        }
        IssueDispatchMode::CommentAndRun => None,
    }
}

/// The §11.2 dedicated full body for `code` (409).
async fn dedicated_code_response(pool: &PgPool, work_item_id: Uuid, code: &str) -> Response {
    let code: &'static str = match code {
        "assigned_runner_offline" => "assigned_runner_offline",
        "assigned_runner_paused" => "assigned_runner_paused",
        "assigned_runner_not_owner" => "assigned_runner_not_owner",
        _ => "assigned_runner_offline",
    };
    use pidash_db::entity::issues;

    let orm_pool = pidash_db::pool::sea_orm_connection(pool);
    let Some(issue) = issues::Entity::find_by_id(work_item_id)
        .one(&orm_pool)
        .await
        .ok()
        .flatten()
    else {
        return server_error();
    };
    match pidash_services::domain::issue_assignment::dedicated_refusal_body(&orm_pool, &issue, code)
        .await
    {
        Ok(body) => json_response(StatusCode::CONFLICT, body.to_string()),
        Err(_) => server_error(),
    }
}

/// The pod_once ref-descriptor (`runs.py` the 403/400/409 bodies).
fn pod_once_code_response(code: &str) -> Response {
    let (status, error) = match code {
        "pod_once_not_allowed" => (
            StatusCode::FORBIDDEN,
            "only the issue's creator and assignees may use pod_once",
        ),
        "pod_once_not_applicable" => (
            StatusCode::BAD_REQUEST,
            "pod_once requires a dedicated runner on the issue",
        ),
        _ => (StatusCode::CONFLICT, "no eligible runner in the pod"),
    };
    json_response(
        status,
        serde_json::to_string(&serde_json::json!({"error": error, "code": code}))
            .expect("serializable refusal"),
    )
}

/// `_post_run_ai` (`runs.py:347-388`).
pub async fn post_run_ai(
    state: &AppState,
    pool: &PgPool,
    actor_id: Uuid,
    body: &Value,
) -> Response {
    post_issue_dispatch(state, pool, actor_id, body, IssueDispatchMode::RunAi).await
}

/// `_post_comment_and_run` (`runs.py:420-466`).
pub async fn post_comment_and_run(
    state: &AppState,
    pool: &PgPool,
    actor_id: Uuid,
    body: &Value,
) -> Response {
    post_issue_dispatch(
        state,
        pool,
        actor_id,
        body,
        IssueDispatchMode::CommentAndRun,
    )
    .await
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn float_repr_matches_cpython() {
        let cases = [
            (1.5, "1.5"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (100.0, "100.0"),
            (0.1, "0.1"),
            (123.456, "123.456"),
            (1.5e20, "1.5e+20"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (-0.0, "-0.0"),
            (0.0, "0.0"),
            (1234567890123456.0, "1234567890123456.0"),
            (2.5e-7, "2.5e-07"),
            (1e21, "1e+21"),
            (std::f64::consts::PI, "3.141592653589793"),
            (2.0, "2.0"),
            (10.0, "10.0"),
            (-1.5e-7, "-1.5e-07"),
            // Shortest-ties where Grisu (`{:?}`) picks a different last
            // digit than CPython; Ryū matches (review, PIDASHCONV-1041).
            (1058518298511673.2, "1058518298511673.2"),
            (-764100621116182.2, "-764100621116182.2"),
            (575579265198138.2, "575579265198138.2"),
            (29750839840935.562, "29750839840935.562"),
        ];
        for (value, expected) in cases {
            assert_eq!(py_float_repr(value), expected, "{value:?}");
        }
    }

    #[test]
    fn str_repr_match_cpython() {
        assert_eq!(py_repr_str("plain"), "'plain'");
        assert_eq!(py_repr_str("a'b"), "\"a'b\"");
        assert_eq!(py_repr_str("a\"b"), "'a\"b'");
        assert_eq!(py_repr_str("a'b\"c"), "'a\\'b\"c'");
        assert_eq!(py_repr_str("x\ny"), "'x\\ny'");
        assert_eq!(py_repr_str("tab\there"), "'tab\\there'");
        assert_eq!(py_str_json(&json!("hi")), "hi");
        assert_eq!(py_str_json(&json!(5)), "5");
        assert_eq!(py_str_json(&json!(true)), "True");
        assert_eq!(py_str_json(&json!(1.5)), "1.5");
        assert_eq!(py_str_json(&json!(1e16)), "1e+16");
        assert_eq!(py_str_json(&json!(null)), "None");
        assert_eq!(py_str_json(&json!([1, 2])), "[1, 2]");
        assert_eq!(py_str_json(&json!({"a": 1})), "{'a': 1}");
        assert_eq!(
            py_str_json(&json!([1, "x", true, null, {"k": [1.5]}])),
            "[1, 'x', True, None, {'k': [1.5]}]"
        );
        // Dicts keep insertion order (`preserve_order` is on).
        let mut map = serde_json::Map::new();
        map.insert("b".to_owned(), json!(1));
        map.insert("a".to_owned(), json!(2));
        assert_eq!(py_str_json(&Value::Object(map)), "{'b': 1, 'a': 2}");
        // Integers render by value, never via `f64` (review,
        // PIDASHCONV-1041): `-0` is `0`, past-`u64` text stays exact.
        assert_eq!(py_str_json(&serde_json::from_str("-0").unwrap()), "0");
        let wide: Value = serde_json::from_str("1267650600228229401496703205376").unwrap();
        assert_eq!(py_str_json(&wide), "1267650600228229401496703205376");
        let neg_wide: Value = serde_json::from_str("-1267650600228229401496703205376").unwrap();
        assert_eq!(py_str_json(&neg_wide), "-1267650600228229401496703205376");
    }

    #[test]
    fn filter_coercion_matches_django() {
        let ws = "9cb6479e-01da-4323-aa82-65bcb556c4f4";
        assert_eq!(coerce_filter_uuid(None), UuidCoerce::Absent);
        assert_eq!(coerce_filter_uuid(Some(&json!(null))), UuidCoerce::Absent);
        assert!(matches!(
            coerce_filter_uuid(Some(&json!(ws))),
            UuidCoerce::Id { raw, .. } if raw == ws
        ));
        assert_eq!(
            coerce_filter_uuid(Some(&json!("nope"))),
            UuidCoerce::Invalid
        );
        assert_eq!(coerce_filter_uuid(Some(&json!(1.5))), UuidCoerce::Invalid);
        assert_eq!(coerce_filter_uuid(Some(&json!([1]))), UuidCoerce::Invalid);
        assert_eq!(
            coerce_filter_uuid(Some(&json!({"a": 1}))),
            UuidCoerce::Invalid
        );
        // Integers ride `UUID(int=)`; negatives fail.
        assert!(matches!(
            coerce_filter_uuid(Some(&json!(123))),
            UuidCoerce::Id { id, raw } if id == Uuid::from_u128(123) && raw == "123"
        ));
        assert_eq!(coerce_filter_uuid(Some(&json!(-1))), UuidCoerce::Invalid);
        assert!(matches!(
            coerce_filter_uuid(Some(&json!(true))),
            UuidCoerce::Id { raw, .. } if raw == "True"
        ));
        // Past-`u64` integers still coerce (Python ints are unbounded).
        let wide: u128 = 1 << 100;
        let wide_json: Value = serde_json::from_str(&wide.to_string()).unwrap();
        assert!(matches!(
            coerce_filter_uuid(Some(&wide_json)),
            UuidCoerce::Id { id, .. } if id == Uuid::from_u128(wide)
        ));
        let too_wide: Value = serde_json::from_str(&(u128::MAX.to_string() + "0")).unwrap();
        assert_eq!(coerce_filter_uuid(Some(&too_wide)), UuidCoerce::Invalid);
    }

    #[test]
    fn capability_conversion_matches_set_semantics() {
        assert_eq!(
            convert_capabilities(&json!(null)).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            convert_capabilities(&json!([])).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            convert_capabilities(&json!("abc")).unwrap(),
            vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]
        );
        assert_eq!(
            convert_capabilities(&json!(["x", "y"])).unwrap(),
            vec!["x".to_owned(), "y".to_owned()]
        );
        assert_eq!(
            convert_capabilities(&json!({"a": 1})).unwrap(),
            vec!["a".to_owned()]
        );
        assert!(convert_capabilities(&json!(5)).is_err());
        assert!(convert_capabilities(&json!([5])).is_err());
        assert!(convert_capabilities(&json!([{"a": 1}])).is_err());
    }

    /// Overlay-shaped seam: a keyless user still counts as configured
    /// (the gateway lane, not BYOK).
    #[derive(Debug)]
    struct GatewaySeam;

    impl pidash_services::extensions::AssistantModelSeam for GatewaySeam {
        fn has_usable_llm_config(
            &self,
            user_id: Uuid,
            has_api_key: bool,
        ) -> pidash_services::extensions::SeamFuture<'_, bool> {
            let _ = (user_id, has_api_key);
            Box::pin(async move { true })
        }

        fn generate_title(
            &self,
            request: pidash_services::extensions::TitleRequest,
        ) -> pidash_services::extensions::SeamFuture<
            '_,
            Result<String, pidash_services::extensions::TitleError>,
        > {
            let _ = request;
            Box::pin(async move { Ok("seam title".to_owned()) })
        }
    }

    #[tokio::test]
    async fn direct_gate_uses_the_installed_seam() {
        let state = AppState::new("test").with_assistant_seam(std::sync::Arc::new(GatewaySeam));
        assert!(
            direct_gate_allows(&state, Uuid::new_v4(), false).await,
            "an installed seam that accepts the user must open the gate without a key"
        );
    }

    #[tokio::test]
    async fn direct_gate_keeps_the_ce_default() {
        // No `with_assistant_seam`: the default CE seam answers the key
        // bit exactly like the pre-seam free function did.
        let state = AppState::new("test");
        assert!(
            direct_gate_allows(&state, Uuid::new_v4(), true).await,
            "CE still admits a keyed creator"
        );
        assert!(
            !direct_gate_allows(&state, Uuid::new_v4(), false).await,
            "CE still refuses a keyless creator (the 409 path)"
        );
    }
}
