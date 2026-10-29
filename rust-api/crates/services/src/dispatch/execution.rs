#![forbid(unsafe_code)]

//! Executor resolve + creation capacity (`creation.py`, `policy.py`).
//!
//! Moved verbatim from `pidash_jobs::dispatch` (PIDASHCONV-1521 Part D):
//! the `execution_fields` resolve (cloud/managed/local), the
//! `lock_cloud_creation_capacity` hard gate, their executor-generic SQL
//! helpers, and the pure verdict cores. The jobs module re-exports every
//! public item, so all `pidash_jobs::dispatch::…` paths keep working.
//! The services home lets the SeaORM-backed creation seam (the domain
//! move path, which cannot depend on jobs) call the resolve directly.
//!
//! Query stream and verdicts are unchanged: this file is a relocation,
//! and its tests still run from the jobs module through the re-exports.

use std::sync::Arc;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::{Map, Value};
use uuid::Uuid;

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
use pidash_db::tx::Transaction;
use pidash_types::dispatch::{AgentExecutorKind, ManagedRunnerReason, ManagedRunnerUnavailable};

use super::admission::AdmissionCache;
use super::admission::CloudAgentAdmissionError;
use super::admission::DeferredConsume;
use super::admission::LlmProfile;
use super::admission::ENROLLED_MANAGED_RUNNERS_EXISTS_SQL;
use super::admission::ENROLLED_PAUSED_MANAGED_RUNNERS_EXISTS_SQL;
use super::admission::ONLINE_MANAGED_RUNNER_SQL;
use super::enforce_creation_rate;
use super::managed_runner_availability;
use super::policy::build_tool_plan;
use super::policy::managed_runner_is_enabled;
use super::policy::resolve_executor_kind;
use super::policy::user_has_llm_config;
use super::policy::CloudAgentUnavailable;
use super::policy::CloudCapabilityUnavailable;
use super::policy::ResolveExecutorError;
use super::policy::UserFlags;
use super::policy::GITHUB_BINDING_EXISTS_SQL;
use super::HEARTBEAT_GRACE_SECS;
use crate::extensions::CloudAgentToolsetsSeam;

/// `_MANAGED_REFUSAL_DETAIL` (`creation.py:135-147`, `09738d222` adds
/// the paused row) with the `.get(reason, ...)` default (`creation.py:132`).
pub fn managed_refusal_detail(reason: &str) -> &'static str {
    match reason {
        "managed_runner_disabled" => "Pi Dash Agent is not enabled on this instance.",
        "desktop_not_connected" => "Open the Pi Dash desktop app on the machine you want this to run on.",
        "llm_config_missing" => {
            "The run creator has no AI provider configured. Configure one in Pi Dash AI settings."
        }
        "gateway_scopes_missing" => "Sign in to Pi Dash again to refresh your AI access.",
        "byok_not_supported_on_desktop" => {
            "Pi Dash Agent on desktop uses OpenHub. Switch your AI provider to OpenHub to run here; \
             Pi Dash AI and the Cloud Agent keep using your own key."
        }
        "no_managed_runner_for_project" => "This project has no Pi Dash Agent on your desktop yet.",
        "managed_runner_paused" => {
            "Your Pi Dash Agent for this project is paused. Resume it to run here."
        }
        _ => "Pi Dash Agent is not available",
    }
}

/// The pinned-runner + waiting-marker half of `_managed_execution_fields`
/// (`creation.py:115-131`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagedFields {
    /// The selected runner id, if the re-query found one (a lost race pins
    /// nothing — Python assigns the `None` too).
    pub pinned_runner_id: Option<Uuid>,
    /// `desktop_not_connected` on the automatic-waiting path only.
    pub error_code: Option<&'static str>,
}

/// Pure branch core of `_managed_execution_fields` (`creation.py:114-132`).
///
/// `available` / `reason` are the `managed_runner_availability` verdict;
/// `enrolled_exists` is the re-queried transient check (`creation.py:123`,
/// evaluated only for `desktop_not_connected`); `online_id` /
/// `latest_enrolled_id` are the re-queried pins (`creation.py:117,125`).
pub fn managed_execution_decision(
    available: bool,
    reason: &str,
    enrolled_exists: bool,
    online_id: Option<Uuid>,
    latest_enrolled_id: Option<Uuid>,
    automatic: bool,
) -> Result<ManagedFields, ManagedRunnerUnavailable> {
    if available {
        return Ok(ManagedFields {
            pinned_runner_id: online_id,
            error_code: None,
        });
    }
    // "Offline right now" is the only transient failure (`creation.py:120-123`).
    let transient = reason == ManagedRunnerReason::NOT_CONNECTED && enrolled_exists;
    if automatic && transient {
        return Ok(ManagedFields {
            pinned_runner_id: latest_enrolled_id,
            error_code: Some(ManagedRunnerReason::NOT_CONNECTED),
        });
    }
    Err(ManagedRunnerUnavailable::new(
        reason,
        managed_refusal_detail(reason),
    ))
}

/// The captured quota shape (`creation.py:70,175`): `{code, detail}` — the
/// `retry_after_seconds` the raised error carries is dropped on capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredAdmissionError {
    pub code: String,
    pub detail: String,
}

/// Capture an admission refusal the way the automatic paths do
/// (`creation.py:51,70,175`): code + `str(exc)`, no retry-after.
pub fn defer_admission_error(exc: &CloudAgentAdmissionError) -> DeferredAdmissionError {
    DeferredAdmissionError {
        code: exc.code().to_owned(),
        detail: exc.detail().to_owned(),
    }
}

/// Pure verdict core of the workspace queue-cap gate (`creation.py:62-70,
/// 163-176`): `Ok(None)` admits, `Ok(Some(_))` defers for automatic runs,
/// `Err(_)` raises for manual ones.
pub fn quota_verdict(
    queued: i64,
    max_queued_per_workspace: i64,
    dispatch_scan_interval_secs: i64,
    automatic: bool,
) -> Result<Option<DeferredAdmissionError>, CloudAgentAdmissionError> {
    if queued < max_queued_per_workspace {
        return Ok(None);
    }
    let error = CloudAgentAdmissionError::new(
        CloudAgentAdmissionError::RUN_QUOTA_EXCEEDED,
        "Cloud Agent queue is full for this workspace",
        Some(dispatch_scan_interval_secs),
    );
    if automatic {
        return Ok(Some(defer_admission_error(&error)));
    }
    Err(error)
}
/// The workspace mutex (`creation.py:56`, `creation.py:162`,
/// `dispatch.py:27`): `Workspace.objects.select_for_update().get(pk=...)`.
/// Projects the constant — the row itself is never read, only locked — and a
/// missing row surfaces as `RowNotFound`, as `.get()` raises. The default
/// manager's `deleted_at IS NULL` guard is load-bearing (a soft-deleted
/// workspace raises instead of locking); the `ORDER BY` the compiler adds is
/// unobservable on a primary-key lookup and omitted. Param: `$1` workspace id
/// (uuid).
pub const WORKSPACE_LOCK_SQL: &str =
    "SELECT 1 FROM workspaces WHERE id = $1 AND deleted_at IS NULL FOR UPDATE";

/// The queued-cloud count (`creation.py:57-61,163-167`): `COUNT(*)` with the
/// workspace first (the `$1` param); the compiler's own conjunct order
/// differs textually — equivalent, since `AND` is commutative. Params: `$1`
/// workspace id (uuid).
pub const QUEUED_CLOUD_COUNT_SQL: &str = "SELECT COUNT(*) FROM agent_run \
    WHERE workspace_id = $1 AND executor_kind = 'cloud_agent' AND status = 'queued'";
/// The automatic-waiting pin (`creation.py:125`):
/// `enrolled_managed_runners(...).order_by("-last_heartbeat_at").first()`.
/// Same `WHERE` shape as L4's [`ENROLLED_MANAGED_RUNNERS_EXISTS_SQL`] plus
/// the bare `DESC` — which sorts never-heartbeated (`NULL`) runners first,
/// exactly as Django does — projecting the id (the pin handle the creation
/// sites pass as `pinned_runner`). Params: `$1` owner id (uuid), `$2`
/// project id (uuid), `$3` workspace id (uuid).
pub const ENROLLED_LATEST_RUNNER_SQL: &str = "SELECT runner.id FROM runner INNER JOIN pod \
    ON pod.id = runner.pod_id WHERE runner.owner_id = $1 \
    AND runner.provisioning = 'desktop_bundled' AND pod.project_id = $2 \
    AND runner.workspace_id = $3 AND runner.revoked_at IS NULL \
    ORDER BY runner.last_heartbeat_at DESC LIMIT 1";
// ---------------------------------------------------------------------------
// Creation seam (`creation.py:11-89`)
// ---------------------------------------------------------------------------

/// The project half of `execution_fields` (`creation.py:14,19,26`): the pod
/// scope the executor resolves under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectScope {
    /// `project.id`.
    pub project_id: Uuid,
    /// `project.workspace_id`.
    pub workspace_id: Uuid,
    /// `project.default_agent_executor` (inherited when `requested` is
    /// `None` or `""`).
    pub default_agent_executor: String,
}

/// The actor half of `execution_fields` (`creation.py:17`): `None` is the
/// anonymous caller (`getattr(actor, "id", None)` → `None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorScope {
    /// `actor.id`.
    pub id: Uuid,
    /// `is_active` / `is_bot` for the LLM-config gate.
    pub flags: UserFlags,
}

/// The inputs `execution_fields` reads off its caller (`creation.py:11-20`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionInputs<'a> {
    /// The project the run is created on.
    pub project: &'a ProjectScope,
    /// `run_kind` (`"issue"`, `"scheduler"`, …).
    pub run_kind: &'a str,
    /// Whether the run binds a work item.
    pub has_issue: bool,
    /// `required_capabilities` (`()` by default).
    pub required_capabilities: &'a [&'a str],
    /// The run creator (`None` for the anonymous caller).
    pub actor: Option<&'a ActorScope>,
    /// Ticker/scheduler runs wait visibly instead of raising on soft gates.
    pub automatic: bool,
    /// The per-issue execution-target override (`Issue.agent_executor`);
    /// `None` inherits the project default.
    pub requested: Option<&'a str>,
    /// `int(time.time())` for the admission buckets.
    pub now_unix_secs: i64,
}

/// The EE-overlayable seams `execution_fields` consults, each only when the
/// Python short-circuit reaches it.
pub struct ExecutionSeams<F, P> {
    /// `has_usable_llm_config(user)` (`agent_execution.py:78-80`): consulted
    /// only for a present, active, non-bot actor on the cloud path.
    pub has_usable_llm_config: F,
    /// `extra_toolsets_enabled_for(creator)` (`toolsets.py:23-32`): the
    /// injected toolsets seam, awaited only when a creator is present.
    pub toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    /// `managed_llm_profile(user)` (`managed_runner/policy.py:27-36`):
    /// consulted only when the managed path reaches the profile gate.
    pub llm_profile: P,
}

/// The executor-specific `AgentRun` fields for one run creation
/// (`execution_fields`, `creation.py:11-89`).
///
/// Consumption mirrors the orchestration call sites: `executor_kind` +
/// `tool_plan` (+ `error_code` when set) splat into `objects.create`;
/// `pinned_runner_id` passes explicitly (the sites `pop` it with a default);
/// `cloud_admission_error` is popped before the splat — `_cloud_admission_error`
/// is a pseudo-key, not an `AgentRun` column (`creation.py:84-85`).
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionFields {
    /// The resolved executor kind.
    pub executor_kind: AgentExecutorKind,
    /// The tool plan (`ToolPlan` JSON on the cloud path, `{}` elsewhere).
    pub tool_plan: Value,
    /// The pin: always `None` on the cloud/local paths, the selected runner
    /// on the managed path.
    pub pinned_runner_id: Option<Uuid>,
    /// `desktop_not_connected` on the managed automatic-waiting path only —
    /// a real `AgentRun` column the sweep reads back (`creation.py:126-131`).
    pub error_code: Option<String>,
    /// The deferred quota refusal on the automatic cloud path (not a column).
    pub cloud_admission_error: Option<DeferredAdmissionError>,
}

/// Every failure `execution_fields` and [`lock_cloud_creation_capacity`]
/// report: the Python raises (`ValueError` / `RuntimeError` at the
/// orchestration call sites) plus the database.
#[derive(Debug, thiserror::Error)]
pub enum ExecutionFieldsError {
    /// `resolve_executor_kind` refused (`creation.py:26`): unknown executor,
    /// cloud not configured, or managed disabled at the instance level.
    #[error(transparent)]
    Resolve(#[from] ResolveExecutorError),
    /// The creator has no LLM config (`creation.py:36-39`).
    #[error(transparent)]
    Unavailable(CloudAgentUnavailable),
    /// A manual gate refused (`creation.py:49-50,68-69`).
    #[error(transparent)]
    Admission(#[from] CloudAgentAdmissionError),
    /// A managed gate refused (`creation.py:132`).
    #[error(transparent)]
    Managed(#[from] ManagedRunnerUnavailable),
    /// A required capability is not plannable (`creation.py:73-81` via
    /// `build_tool_plan`).
    #[error(transparent)]
    Capability(#[from] CloudCapabilityUnavailable),
    /// The database failed.
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// The workspace mutex take, shared with the dispatch engine: `lock_workspace`
/// runs on the caller-provided executor (span or short transaction).
pub async fn lock_workspace(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    workspace_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query_scalar::<_, i32>(WORKSPACE_LOCK_SQL)
        .bind(workspace_id)
        .fetch_one(executor)
        .await?;
    Ok(())
}

async fn count_queued_cloud(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    workspace_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(QUEUED_CLOUD_COUNT_SQL)
        .bind(workspace_id)
        .fetch_one(executor)
        .await
}

async fn github_binding_exists(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    project: &ProjectScope,
) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar::<_, i32>(GITHUB_BINDING_EXISTS_SQL)
        .bind(project.project_id)
        .bind(project.workspace_id)
        .fetch_optional(executor)
        .await?;
    Ok(found.is_some())
}

async fn enrolled_exists(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    project: &ProjectScope,
    owner_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar::<_, i32>(ENROLLED_MANAGED_RUNNERS_EXISTS_SQL)
        .bind(owner_id)
        .bind(project.project_id)
        .bind(project.workspace_id)
        .fetch_optional(executor)
        .await?;
    Ok(found.is_some())
}

async fn online_managed_runner_id(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    project: &ProjectScope,
    owner_id: Uuid,
    heartbeat_threshold: DateTime<Utc>,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar::<_, Uuid>(ONLINE_MANAGED_RUNNER_SQL)
        .bind(owner_id)
        .bind(project.project_id)
        .bind(project.workspace_id)
        .bind(heartbeat_threshold)
        .fetch_optional(executor)
        .await
}

/// The paused tiebreaker (`policy.py:88-91`, `09738d222`): whether the
/// viewer owns any paused bundled runner on the project's pod.
async fn paused_enrolled_exists(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    project: &ProjectScope,
    owner_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar::<_, i32>(ENROLLED_PAUSED_MANAGED_RUNNERS_EXISTS_SQL)
        .bind(owner_id)
        .bind(project.project_id)
        .bind(project.workspace_id)
        .fetch_optional(executor)
        .await?;
    Ok(found.is_some())
}

async fn latest_enrolled_runner_id(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    project: &ProjectScope,
    owner_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar::<_, Uuid>(ENROLLED_LATEST_RUNNER_SQL)
        .bind(owner_id)
        .bind(project.project_id)
        .bind(project.workspace_id)
        .fetch_optional(executor)
        .await
}

/// Resolve the executor-specific `AgentRun` fields for one run creation
/// (`execution_fields`, `creation.py:11-89`).
///
/// The capacity gate runs on the caller's transaction — the ambient
/// `atomic` at `creation.py:55-70` made explicit. Orchestration callers
/// (D-12) open a short transaction that commits before the fields are built
/// (today's inner-atomic shape); the direct-run view
/// (`runner/views/runs.py:262-264`) passes its insertion transaction, so the
/// workspace lock is held across gate+insert exactly as the savepoint does in
/// Python and the queue cap stays a hard cap for concurrent creates. The
/// deferred admission consumes collect into `on_commit` and the caller runs
/// them after commit — a rolled-back creation burns no quota, while consumes
/// registered before a later raise stay registered, as on Django's ambient
/// transaction. This function never commits or rolls back `tx` itself.
#[allow(clippy::too_many_arguments)]
pub async fn execution_fields<C, F, P>(
    tx: &mut Transaction<'_>,
    inputs: &ExecutionInputs<'_>,
    cloud: &CloudAgentSettings,
    managed: &ManagedRunnerSettings,
    cache: &C,
    on_commit: &mut (dyn FnMut(DeferredConsume) + Send),
    seams: ExecutionSeams<F, P>,
) -> Result<ExecutionFields, ExecutionFieldsError>
where
    C: AdmissionCache,
    F: FnOnce() -> bool,
    P: FnOnce() -> LlmProfile,
{
    let executor = resolve_executor_kind(
        inputs.requested,
        &inputs.project.default_agent_executor,
        cloud,
        managed,
    )?;
    if executor == AgentExecutorKind::CloudAgent {
        return execution_fields_cloud(tx.inner(), inputs, cloud, cache, on_commit, seams).await;
    }
    if executor == AgentExecutorKind::ManagedRunner {
        return execution_fields_managed(tx.inner(), inputs, managed, seams.llm_profile).await;
    }
    Ok(ExecutionFields {
        executor_kind: executor,
        tool_plan: Value::Object(Map::new()),
        pinned_runner_id: None,
        error_code: None,
        cloud_admission_error: None,
    })
}

async fn execution_fields_cloud<C, F, P>(
    conn: &mut sqlx::PgConnection,
    inputs: &ExecutionInputs<'_>,
    cloud: &CloudAgentSettings,
    cache: &C,
    on_commit: &mut (dyn FnMut(DeferredConsume) + Send),
    seams: ExecutionSeams<F, P>,
) -> Result<ExecutionFields, ExecutionFieldsError>
where
    C: AdmissionCache,
    F: FnOnce() -> bool,
    P: FnOnce() -> LlmProfile,
{
    let actor_flags = inputs.actor.map(|actor| &actor.flags);
    // A run without a funded principal can never start (`creation.py:33-39`).
    if !user_has_llm_config(actor_flags, seams.has_usable_llm_config) {
        return Err(ExecutionFieldsError::Unavailable(
            CloudAgentUnavailable::new(
                "The run creator has no AI provider configured. Configure one in Pi Dash AI settings.",
            ),
        ));
    }
    let actor_id = inputs.actor.map(|actor| actor.id.to_string());
    let mut admission_error = None;
    if let Err(exc) = enforce_creation_rate(
        cloud,
        inputs.now_unix_secs,
        &inputs.project.workspace_id.to_string(),
        actor_id.as_deref(),
        inputs.automatic,
        cache,
        on_commit,
    ) {
        if !inputs.automatic {
            return Err(ExecutionFieldsError::Admission(exc));
        }
        admission_error = Some(defer_admission_error(&exc));
    }
    // The point-in-time gate on the caller's transaction (`creation.py:52-70`):
    // orchestration callers commit their short transaction after this returns
    // (releasing the lock before the fields are built, as the inner atomic
    // does), while the direct-run view holds the lock across gate+insert.
    // Either way the insertion transaction re-checks via
    // `lock_cloud_creation_capacity`.
    lock_workspace(&mut *conn, inputs.project.workspace_id).await?;
    let queued = count_queued_cloud(&mut *conn, inputs.project.workspace_id).await?;
    match quota_verdict(
        queued,
        cloud.max_queued_per_workspace,
        cloud.dispatch_scan_interval_secs,
        inputs.automatic,
    )? {
        None => {}
        Some(deferred) => admission_error = Some(deferred),
    }
    // The github leg runs only with the kill switch on (the `policy.py:119`
    // short-circuit); the project is always present here.
    let github_available = if cloud.github_tools_enabled {
        Some(github_binding_exists(&mut *conn, inputs.project).await?)
    } else {
        None
    };
    // The run executes as its creator, so their preference is what the
    // snapshot records (`creation.py:78-80`); no creator, no seam consult.
    let extra_toolsets = match inputs.actor {
        Some(actor) => {
            let verdict = seams.toolsets.extra_toolsets_enabled(actor.id).await;
            Some(move || verdict)
        }
        None => None,
    };
    let plan = build_tool_plan(
        inputs.run_kind,
        inputs.has_issue,
        inputs.required_capabilities,
        github_available.map(|available| move || available),
        extra_toolsets,
        cloud,
    )?;
    Ok(ExecutionFields {
        executor_kind: AgentExecutorKind::CloudAgent,
        tool_plan: serde_json::to_value(&plan).expect("ToolPlan serializes to JSON"),
        pinned_runner_id: None,
        error_code: None,
        cloud_admission_error: admission_error,
    })
}

async fn execution_fields_managed<P>(
    conn: &mut sqlx::PgConnection,
    inputs: &ExecutionInputs<'_>,
    managed: &ManagedRunnerSettings,
    llm_profile: P,
) -> Result<ExecutionFields, ExecutionFieldsError>
where
    P: FnOnce() -> LlmProfile,
{
    let user = inputs.actor.map(|actor| &actor.flags);
    // Gate-order mirror (`managed_runner/policy.py:76-96`): the kill switch,
    // viewer-validity and profile gates return before any query, so the
    // enrolled/online/paused reads run only when every earlier gate passes —
    // the online read only when enrolled holds (`:92-95`), the paused probe
    // only when enrolled holds but nothing is online (`09738d222`). The
    // stashed profile feeds the L4 call below, so the seam is still consulted
    // exactly once, exactly when the path reaches it.
    let pre_gates_pass = managed_runner_is_enabled(managed)
        && matches!(user, Some(flags) if flags.is_active && !flags.is_bot);
    let profile = pre_gates_pass.then(llm_profile);
    let profile_available = profile.as_ref().is_some_and(|profile| profile.available);
    let (enrolled, online, paused) = match (inputs.actor, profile_available) {
        (Some(actor), true) => {
            let enrolled = enrolled_exists(&mut *conn, inputs.project, actor.id).await?;
            let online = if enrolled {
                let threshold = Utc::now() - ChronoDuration::seconds(HEARTBEAT_GRACE_SECS);
                online_managed_runner_id(&mut *conn, inputs.project, actor.id, threshold)
                    .await?
                    .is_some()
            } else {
                false
            };
            // `09738d222`: the paused probe runs only when a runner is
            // enrolled but none is online.
            let paused = if enrolled && !online {
                paused_enrolled_exists(&mut *conn, inputs.project, actor.id).await?
            } else {
                false
            };
            (enrolled, online, paused)
        }
        (None, _) | (Some(_), false) => (false, false, false),
    };
    // The fallback profile is dead on arrival: L4 re-evaluates these same
    // gates and returns before consulting it on every path where `profile` is
    // `None`. Outcomes are identical to the un-mirrored call; only the query
    // stream changes (0-3 reads instead of always 2).
    let verdict = managed_runner_availability(
        managed,
        user,
        || {
            profile.unwrap_or(LlmProfile {
                available: false,
                reason_code: String::new(),
            })
        },
        enrolled,
        online,
        paused,
    );
    // The re-queries (`creation.py:117,123,125`): the available path and the
    // transient check re-read rather than reuse the verdicts.
    let online_id = if verdict.available {
        match inputs.actor {
            Some(actor) => {
                let threshold = Utc::now() - ChronoDuration::seconds(HEARTBEAT_GRACE_SECS);
                online_managed_runner_id(&mut *conn, inputs.project, actor.id, threshold).await?
            }
            None => None,
        }
    } else {
        None
    };
    let transient_enrolled = verdict.reason_code == ManagedRunnerReason::NOT_CONNECTED
        && match inputs.actor {
            Some(actor) => enrolled_exists(&mut *conn, inputs.project, actor.id).await?,
            None => false,
        };
    let latest_enrolled_id = if inputs.automatic && transient_enrolled {
        match inputs.actor {
            Some(actor) => latest_enrolled_runner_id(&mut *conn, inputs.project, actor.id).await?,
            None => None,
        }
    } else {
        None
    };
    let fields = managed_execution_decision(
        verdict.available,
        &verdict.reason_code,
        transient_enrolled,
        online_id,
        latest_enrolled_id,
        inputs.automatic,
    )?;
    Ok(ExecutionFields {
        executor_kind: AgentExecutorKind::ManagedRunner,
        tool_plan: Value::Object(Map::new()),
        pinned_runner_id: fields.pinned_runner_id,
        error_code: fields.error_code.map(str::to_string),
        cloud_admission_error: None,
    })
}

/// Repeat hard admission while holding the caller's insertion transaction
/// (`lock_cloud_creation_capacity`, `creation.py:155-177`).
///
/// Non-cloud executors return `Ok(None)` without touching the database.
/// Over capacity, automatic runs defer (`Ok(Some(_))`) and manual runs raise —
/// and the caller folds the deferral over any earlier capture
/// (`lock(...) or admission_error` at the orchestration sites).
pub async fn lock_cloud_creation_capacity(
    tx: &mut Transaction<'_>,
    workspace_id: Uuid,
    executor_kind: &AgentExecutorKind,
    automatic: bool,
    cloud: &CloudAgentSettings,
) -> Result<Option<DeferredAdmissionError>, ExecutionFieldsError> {
    if *executor_kind != AgentExecutorKind::CloudAgent {
        return Ok(None);
    }
    lock_workspace(&mut **tx.inner(), workspace_id).await?;
    let queued = count_queued_cloud(&mut **tx.inner(), workspace_id).await?;
    Ok(quota_verdict(
        queued,
        cloud.max_queued_per_workspace,
        cloud.dispatch_scan_interval_secs,
        automatic,
    )?
    .into_iter()
    .next())
}
