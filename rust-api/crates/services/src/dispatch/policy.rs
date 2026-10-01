#![forbid(unsafe_code)]

//! Executor policy: cloud_agent + managed_runner L3 (D-11, stage 5).
//!
//! Port of the policy half of `apps/api/pi_dash/core/agent_execution.py:34-128`
//! and all of `apps/api/pi_dash/cloud_agent/policy.py:11-196`:
//!
//! * `cloud_agent_is_configured` (`agent_execution.py:34-43`) → [`cloud_agent_is_configured`].
//! * `managed_runner_is_enabled` (`agent_execution.py:46-54`) → [`managed_runner_is_enabled`].
//! * `effective_executor_for_issue` (`agent_execution.py:57-66`) → [`effective_executor_for_issue`].
//! * `user_has_llm_config` (`agent_execution.py:69-80`) → [`user_has_llm_config`].
//! * `agent_executor_options` (`agent_execution.py:83-128`) → [`agent_executor_options`]
//!   + [`LOCAL_RUNNER_EXISTS_SQL`].
//! * `READ_TOOLS` / `WRITE_TOOLS` (`policy.py:11-29`) → [`READ_TOOLS`] / [`WRITE_TOOLS`].
//! * `REPEATABLE_WRITE_TOOLS` / `CURRENT_ISSUE_WRITE_TOOLS` (`policy.py:30-42`) →
//!   [`REPEATABLE_WRITE_TOOLS`] / [`CURRENT_ISSUE_WRITE_TOOLS`].
//! * `CloudAgentUnavailable` / `CloudCapabilityUnavailable` / `RequiredToolUnavailable`
//!   (`policy.py:45-54`) → [`CloudAgentUnavailable`] / [`CloudCapabilityUnavailable`] /
//!   [`RequiredToolUnavailable`].
//! * `resolve_executor_kind` (`policy.py:57-72`) → [`resolve_executor_kind`].
//! * `github_available_for_project` (`policy.py:75-94`) → [`github_available_for_project`]
//!   + [`GITHUB_BINDING_EXISTS_SQL`].
//! * `build_tool_plan` (`policy.py:97-159`) → [`build_tool_plan`] + [`ToolPlan`].
//! * `resolve_current_tool_names` (`policy.py:162-196`) → [`resolve_current_tool_names`]
//!   + [`resolve_run_project`] + [`BINDING_REPO_SQL`] + [`CODE_REVIEW_LINK_EXISTS_SQL`].
//!
//! Out of scope here (owned by siblings): `AgentExecutorKind` / `MACHINE_EXECUTORS` /
//! `get_default_agent_executor` (L1, `pidash_types::dispatch`); `ManagedRunnerReason` /
//! `ManagedRunnerUnavailable` (L1, reused, not redefined);
//! `CloudAgentUnavailableAPI` (`cloud_agent/api.py`, L4 PIDASHCONV-485);
//! `managed_runner_availability` (`managed_runner/policy.py`, L4 — arrives here as a
//! precomputed [`ManagedAvailability`]); the `extra_toolsets_enabled_for` CE default
//! (`ee/cloud_agent/toolsets.py`, L5 PIDASHCONV-486 — arrives as a seam verdict).
//!
//! Translation notes:
//!
//! * Django settings arrive as the already-ported central structs
//!   (`pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings}`); no
//!   parallel settings struct is defined. The kill-switch getters stay as named
//!   functions so call sites keep one switch point instead of reaching into fields.
//! * Limits are `i64`, the central-`Settings` precedent for env-parsed ints.
//! * EE-overlayable seams and DB verdicts arrive as inputs, never as reimplemented
//!   logic: `has_usable_llm_config` (`agent_execution.py:78-80`) and the
//!   project-github check (`policy.py:119-122`) arrive as `FnOnce` closures so the
//!   Python short-circuit structure (seam consulted only when reached) is preserved
//!   and provable in tests; the local-runner verdict and the managed verdict arrive
//!   as precomputed values whose SQL is owned here ([`LOCAL_RUNNER_EXISTS_SQL`]) or
//!   by L4 respectively.
//! * `requested or default` (`policy.py:58`) and `getattr(issue, ...) or default`
//!   (`agent_execution.py:66`) treat `""` as missing (Semantic traps: truthiness of
//!   empty strings); both ports filter empty strings, verified against Django.
//! * `resolve_executor_kind` returns the [`AgentExecutorKind`] (L1), whose `Display` /
//!   serde form is the value Python returns as `str`.
//! * `ToolPlan` / [`ToolPlanLimits`] / [`ExecutorOption`] declare fields in Python dict
//!   order so serialized JSON keeps the key order byte for byte.
//! * SQL builders emit text with Postgres `$n` placeholders; the caller splices them
//!   into its sqlx statement (services convention). Fixed enum values are literals,
//!   caller-supplied ids are `$n` params, each documented on its const.
//!
//! Fixture: `rust-api/fixtures/dispatch/fx-disp-03-policy.golden.json` (FX-DISP-03).
//!
//! Ported bugs: none found in these units on read-through (plus a Django-oracle pass
//! over the edge semantics: duplicate required capabilities, `""` overrides, catalog
//! contents, plan key order).

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
use pidash_types::dispatch::{AgentExecutorKind, ManagedRunnerReason, ManagedRunnerUnavailable};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Tool catalogs (`policy.py:11-42`)
// ---------------------------------------------------------------------------

/// Read tools, in `policy.py:11-21` order.
pub const READ_TOOLS: [&str; 9] = [
    "pidash_get_current_issue",
    "pidash_list_current_issue_comments",
    "pidash_list_project_states",
    "pidash_search_project_issues",
    "pidash_get_project_issue",
    "pidash_list_linked_code_reviews",
    "pidash_list_issue_relations",
    "github_get_file",
    "github_get_linked_pull_request",
];

/// Write tools, in `policy.py:22-29` order.
pub const WRITE_TOOLS: [&str; 6] = [
    "pidash_add_current_issue_comment",
    "pidash_update_current_issue_workpad",
    "pidash_transition_current_issue",
    "pidash_create_project_issue",
    "pidash_relate_issues",
    "pidash_unrelate_issues",
];

/// Idempotent-by-construction writes a run may call more than once
/// (`policy.py:30-34`). Consumed by L5 (`tools.py:105`); defined here because this
/// is its Python home. Literal order kept.
pub const REPEATABLE_WRITE_TOOLS: [&str; 2] = ["pidash_relate_issues", "pidash_unrelate_issues"];

/// Writes aimed at the run's bound issue (`policy.py:35-42`). Literal order kept.
pub const CURRENT_ISSUE_WRITE_TOOLS: [&str; 3] = [
    "pidash_add_current_issue_comment",
    "pidash_update_current_issue_workpad",
    "pidash_transition_current_issue",
];

/// The two github tools, named once for the two sites that strip them together
/// (`policy.py:122` in `build_tool_plan`, `policy.py:176` in
/// `resolve_current_tool_names`).
pub const GITHUB_TOOLS: [&str; 2] = ["github_get_file", "github_get_linked_pull_request"];

/// Tools dropped when the run has no bound issue (`policy.py:107-115`), verbatim.
/// (The three write names are no-ops here — the set holds reads only at that point —
/// and are stripped again from the writes union at `policy.py:127-128`; the order of
/// operations below mirrors Python exactly rather than folding the two sites.)
const NO_ISSUE_STRIPPED: [&str; 7] = [
    "pidash_get_current_issue",
    "pidash_list_current_issue_comments",
    "pidash_list_linked_code_reviews",
    "github_get_linked_pull_request",
    "pidash_add_current_issue_comment",
    "pidash_update_current_issue_workpad",
    "pidash_transition_current_issue",
];

// ---------------------------------------------------------------------------
// Kill switches (`agent_execution.py:34-54`)
// ---------------------------------------------------------------------------

/// Whether the managed executor can accept new work: the operator kill switch
/// (`agent_execution.py:34-43`).
pub fn cloud_agent_is_configured(cloud: &CloudAgentSettings) -> bool {
    cloud.enabled
}

/// Operator kill switch for the desktop-bundled managed runner
/// (`agent_execution.py:46-54`).
pub fn managed_runner_is_enabled(managed: &ManagedRunnerSettings) -> bool {
    managed.enabled
}

// ---------------------------------------------------------------------------
// Policy errors (`policy.py:45-54`)
// ---------------------------------------------------------------------------

/// Cloud Agent unavailable (`policy.py:45-46`; a `ValueError` in Python).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudAgentUnavailable {
    message: String,
}

impl CloudAgentUnavailable {
    /// The `code` class attribute (`policy.py:46`).
    pub const CODE: &'static str = "cloud_agent_unavailable";

    /// `CloudAgentUnavailable(message)`.
    pub fn new(message: impl Into<String>) -> Self {
        CloudAgentUnavailable {
            message: message.into(),
        }
    }

    /// The carried message (`str(exc)`).
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The reason code.
    pub fn code(&self) -> &'static str {
        Self::CODE
    }
}

impl std::fmt::Display for CloudAgentUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CloudAgentUnavailable {}

/// Required capabilities the plan cannot provide (`policy.py:49-50`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudCapabilityUnavailable {
    message: String,
}

impl CloudCapabilityUnavailable {
    /// The `code` class attribute (`policy.py:50`).
    pub const CODE: &'static str = "cloud_capability_unavailable";

    /// `CloudCapabilityUnavailable(message)`.
    pub fn new(message: impl Into<String>) -> Self {
        CloudCapabilityUnavailable {
            message: message.into(),
        }
    }

    /// The carried message (`str(exc)`).
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The reason code.
    pub fn code(&self) -> &'static str {
        Self::CODE
    }
}

impl std::fmt::Display for CloudCapabilityUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CloudCapabilityUnavailable {}

/// A plan-required tool withdrawn mid-flight (`policy.py:53-54`; a `RuntimeError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredToolUnavailable {
    message: String,
}

impl RequiredToolUnavailable {
    /// The `code` class attribute (`policy.py:54`).
    pub const CODE: &'static str = "required_tool_unavailable";

    /// `RequiredToolUnavailable(message)`.
    pub fn new(message: impl Into<String>) -> Self {
        RequiredToolUnavailable {
            message: message.into(),
        }
    }

    /// The carried message (`str(exc)`).
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The reason code.
    pub fn code(&self) -> &'static str {
        Self::CODE
    }
}

impl std::fmt::Display for RequiredToolUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RequiredToolUnavailable {}

/// The three `resolve_executor_kind` failure modes (`policy.py:57-72`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveExecutorError {
    /// `ValueError("unknown agent executor")` (`policy.py:60`); a bare `ValueError`
    /// with no `code` attribute.
    UnknownExecutor,
    /// `CloudAgentUnavailable` (`policy.py:61-62`).
    CloudAgent(CloudAgentUnavailable),
    /// `ManagedRunnerUnavailable(DISABLED, ...)` (`policy.py:63-71`).
    ManagedRunner(ManagedRunnerUnavailable),
}

impl std::fmt::Display for ResolveExecutorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveExecutorError::UnknownExecutor => write!(f, "unknown agent executor"),
            ResolveExecutorError::CloudAgent(err) => write!(f, "{err}"),
            ResolveExecutorError::ManagedRunner(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ResolveExecutorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ResolveExecutorError::UnknownExecutor => None,
            ResolveExecutorError::CloudAgent(err) => Some(err),
            ResolveExecutorError::ManagedRunner(err) => Some(err),
        }
    }
}

/// Routing matrix, verbatim (`policy.py:57-72`).
///
/// `requested` is `None` when no override was given; `Some("")` falls through to
/// the default exactly like `None` (`requested or ...`, `policy.py:58`).
pub fn resolve_executor_kind(
    requested: Option<&str>,
    project_default: &str,
    cloud: &CloudAgentSettings,
    managed: &ManagedRunnerSettings,
) -> Result<AgentExecutorKind, ResolveExecutorError> {
    let value = requested
        .filter(|r| !r.is_empty())
        .unwrap_or(project_default);
    let kind = AgentExecutorKind::from_value(value).ok_or(ResolveExecutorError::UnknownExecutor)?;
    if kind == AgentExecutorKind::CloudAgent && !cloud_agent_is_configured(cloud) {
        return Err(ResolveExecutorError::CloudAgent(
            CloudAgentUnavailable::new("Pi Dash Cloud Agent is not currently available"),
        ));
    }
    if kind == AgentExecutorKind::ManagedRunner && !managed_runner_is_enabled(managed) {
        // Instance-level only. Whether *this* viewer's desktop can take the run
        // is a per-viewer question answered in `execution_fields` (L6).
        return Err(ResolveExecutorError::ManagedRunner(
            ManagedRunnerUnavailable::new(
                ManagedRunnerReason::DISABLED,
                "Pi Dash Agent is not enabled on this instance",
            ),
        ));
    }
    Ok(kind)
}

// ---------------------------------------------------------------------------
// Per-issue / per-user policy (`agent_execution.py:57-80`)
// ---------------------------------------------------------------------------

/// The executor in force for an issue: the per-issue override wins, `None` (or `""`)
/// inherits the project default (`agent_execution.py:57-66`).
pub fn effective_executor_for_issue<'a>(
    issue_executor: Option<&'a str>,
    project_default: &'a str,
) -> &'a str {
    match issue_executor {
        Some(value) if !value.is_empty() => value,
        _ => project_default,
    }
}

/// The user flags `user_has_llm_config` reads (`agent_execution.py:76`).
///
/// A missing attribute in Python reads as `False` (`getattr` defaults), so a caller
/// mapping a partial user passes `false` for the unknown flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserFlags {
    /// `user.is_active`.
    pub is_active: bool,
    /// `user.is_bot`.
    pub is_bot: bool,
}

/// Whether `user` has a usable LLM config (`agent_execution.py:69-80`).
///
/// `None` / inactive / bot short-circuit to `false` without consulting the
/// `has_usable_llm_config` EE seam; otherwise the seam verdict is returned as-is
/// (CE default path: BYOK key presence, owned by the assistant domain).
pub fn user_has_llm_config<F>(user: Option<&UserFlags>, has_usable_llm_config: F) -> bool
where
    F: FnOnce() -> bool,
{
    match user {
        None => false,
        Some(flags) if !flags.is_active || flags.is_bot => false,
        Some(_) => has_usable_llm_config(),
    }
}

// ---------------------------------------------------------------------------
// Executor options (`agent_execution.py:83-128`)
// ---------------------------------------------------------------------------

/// `agent_execution.py:98`: cloud on, but the viewer has no LLM config.
pub const LLM_CONFIG_MISSING_REASON: &str = "llm_config_missing";

/// `agent_execution.py:121`: no online non-bundled runner on the project's pod.
pub const NO_LOCAL_RUNNER_REASON: &str = "no_local_runner";

/// One `agent_executor_options` row (`agent_execution.py:112-128`), fields in dict
/// order (`kind`, `available`, `reason_code`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutorOption {
    pub kind: AgentExecutorKind,
    pub available: bool,
    pub reason_code: String,
}

/// The `(available, reason_code)` verdict of L4's `managed_runner_availability`
/// (`managed_runner/policy.py:76-96`), passed through verbatim
/// (`agent_execution.py:111`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedAvailability {
    pub available: bool,
    pub reason_code: String,
}

/// Availability as explicit API data (`agent_execution.py:83-128`).
///
/// Rows are always `[cloud_agent, local_runner, managed_runner]`. The cloud row also
/// reflects the viewer's LLM config when `user` is given; without a user only the
/// instance switch is reported. `local_runner_exists` is the [`LOCAL_RUNNER_EXISTS_SQL`]
/// verdict; `managed` is L4's `managed_runner_availability` verdict, passed through
/// verbatim.
pub fn agent_executor_options<F>(
    cloud: &CloudAgentSettings,
    user: Option<UserFlags>,
    has_usable_llm_config: F,
    local_runner_exists: bool,
    managed: ManagedAvailability,
) -> [ExecutorOption; 3]
where
    F: FnOnce() -> bool,
{
    let mut cloud_available = cloud_agent_is_configured(cloud);
    let mut cloud_reason = if cloud_available {
        String::new()
    } else {
        CloudAgentUnavailable::CODE.to_string()
    };
    if cloud_available
        && user.is_some()
        && !user_has_llm_config(user.as_ref(), has_usable_llm_config)
    {
        cloud_available = false;
        cloud_reason = LLM_CONFIG_MISSING_REASON.to_string();
    }
    [
        ExecutorOption {
            kind: AgentExecutorKind::CloudAgent,
            available: cloud_available,
            reason_code: cloud_reason,
        },
        ExecutorOption {
            kind: AgentExecutorKind::LocalRunner,
            available: local_runner_exists,
            reason_code: if local_runner_exists {
                String::new()
            } else {
                NO_LOCAL_RUNNER_REASON.to_string()
            },
        },
        ExecutorOption {
            kind: AgentExecutorKind::ManagedRunner,
            available: managed.available,
            reason_code: managed.reason_code,
        },
    ]
}

/// The local-runner leg of `agent_executor_options` (`agent_execution.py:102-110`):
/// an ONLINE runner on the project's pod, desktop-bundled runners excluded (they
/// serve only runs pinned to them, so counting one would advertise availability for
/// work it never accepts).
///
/// Django renders the `.exclude()` as `NOT (provisioning = ...)`; `<>` is identical
/// because `provisioning` is `NOT NULL`. No `deleted_at` scope: `runner` has no such
/// column, and Django never filters joined tables (`pod`) by their managers.
/// Params: `$1` project id (uuid), `$2` workspace id (uuid).
pub const LOCAL_RUNNER_EXISTS_SQL: &str = "SELECT 1 FROM runner INNER JOIN pod ON pod.id = runner.pod_id WHERE pod.project_id = $1 AND runner.workspace_id = $2 AND runner.status = 'online' AND runner.provisioning <> 'desktop_bundled' LIMIT 1";

// ---------------------------------------------------------------------------
// GitHub availability (`policy.py:75-94`)
// ---------------------------------------------------------------------------

/// The binding leg of `github_available_for_project` (`policy.py:80-94`).
///
/// Four `INNER JOIN`s mirroring Django's join chain for the
/// `provider_account__workspace_integration__github_app_installation` span (the
/// intermediate `workspace_integrations` join is required: a `NULL`
/// `workspace_integration_id` must not match). No `deleted_at` scope on the joined
/// tables — Django filters only the base table. Predicates follow the `filter()`
/// kwarg order. Params: `$1` project id (uuid), `$2` workspace id (uuid).
pub const GITHUB_BINDING_EXISTS_SQL: &str = "SELECT 1 FROM git_repository_bindings b INNER JOIN git_repositories r ON r.id = b.repository_id INNER JOIN git_provider_accounts a ON a.id = b.provider_account_id INNER JOIN workspace_integrations w ON w.id = a.workspace_integration_id INNER JOIN github_app_installations i ON i.workspace_integration_id = w.id WHERE b.project_id = $1 AND b.workspace_id = $2 AND b.deleted_at IS NULL AND r.provider = 'github' AND r.host_url = 'https://github.com' AND a.workspace_id = $2 AND a.provider = 'github' AND a.host_url = 'https://github.com' AND a.auth_type = 'github_app' AND a.status = 'connected' AND a.verified_at IS NOT NULL AND i.verified_at IS NOT NULL AND i.suspended_at IS NULL LIMIT 1";

/// Whether the project's GitHub tooling is usable (`policy.py:75-94`).
///
/// The kill switch short-circuits: when off, `binding_exists` (the
/// [`GITHUB_BINDING_EXISTS_SQL`] verdict) is never consulted and no DB is touched.
pub fn github_available_for_project<F>(cloud: &CloudAgentSettings, binding_exists: F) -> bool
where
    F: FnOnce() -> bool,
{
    if !cloud.github_tools_enabled {
        return false;
    }
    binding_exists()
}

// ---------------------------------------------------------------------------
// Tool plan (`policy.py:97-159`)
// ---------------------------------------------------------------------------

/// `ToolPlan.limits` (`policy.py:140-148`), keys in dict order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolPlanLimits {
    pub model_requests: i64,
    pub tool_calls: i64,
    pub writes: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub wall_seconds: i64,
}

/// Immutable tool plan (`policy.py:135-159`), keys in dict order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolPlan {
    pub v: i32,
    pub catalog_version: i32,
    pub tools: Vec<String>,
    pub required_tools: Vec<String>,
    pub limits: ToolPlanLimits,
    pub unavailable_capabilities: Vec<String>,
    pub extra_toolsets: bool,
}

/// Plan envelope version (`policy.py:136`).
pub const TOOL_PLAN_VERSION: i32 = 1;

/// Tool catalog version (`policy.py:137`).
pub const TOOL_CATALOG_VERSION: i32 = 1;

/// Capabilities no Cloud Agent run ever has (`policy.py:149`).
pub const UNAVAILABLE_CAPABILITIES: [&str; 3] = ["filesystem", "shell", "worktree"];

/// Immutable tool-plan construction (`policy.py:97-159`).
///
/// `project_github_available` is `None` when there is no project (the DB-backed github
/// gate is skipped, `policy.py:119-122`); `Some(check)` is consulted only when the
/// github kill switch is on. `creator_extra_toolsets` is `None` when there is no
/// creator; `Some(check)` carries the EE `extra_toolsets_enabled_for` seam verdict,
/// snapshotted onto the plan as a sibling of `tools` — never a member — so a
/// mid-flight preference flip applies to the next run, not this one.
pub fn build_tool_plan<G, E>(
    run_kind: &str,
    has_issue: bool,
    required_capabilities: &[&str],
    project_github_available: Option<G>,
    creator_extra_toolsets: Option<E>,
    settings: &CloudAgentSettings,
) -> Result<ToolPlan, CloudCapabilityUnavailable>
where
    G: FnOnce() -> bool,
    E: FnOnce() -> bool,
{
    let disabled: HashSet<&str> = settings.disabled_tools.iter().map(String::as_str).collect();
    let mut tools: HashSet<&str> = READ_TOOLS.into_iter().collect();
    if !has_issue {
        for tool in NO_ISSUE_STRIPPED {
            tools.remove(tool);
        }
    }
    if run_kind != "scheduler" {
        tools.remove("pidash_get_project_issue");
        tools.remove("pidash_create_project_issue");
    }
    let github_stripped = if !settings.github_tools_enabled {
        true
    } else if let Some(check) = project_github_available {
        !check()
    } else {
        false
    };
    if github_stripped {
        for tool in GITHUB_TOOLS {
            tools.remove(tool);
        }
    }
    if settings.writes_enabled {
        tools.extend(WRITE_TOOLS);
        if run_kind != "scheduler" {
            tools.remove("pidash_create_project_issue");
        }
        if !has_issue {
            for tool in CURRENT_ISSUE_WRITE_TOOLS {
                tools.remove(tool);
            }
        }
    }
    for tool in &disabled {
        tools.remove(tool);
    }
    let requested: BTreeSet<&str> = required_capabilities.iter().copied().collect();
    let unavailable: Vec<&str> = requested
        .iter()
        .filter(|tool| !tools.contains(*tool))
        .copied()
        .collect();
    if !unavailable.is_empty() {
        return Err(CloudCapabilityUnavailable::new(format!(
            "Cloud Agent cannot provide required capabilities: {}",
            unavailable.join(", ")
        )));
    }
    let required: Vec<String> = requested.into_iter().map(str::to_string).collect();
    let mut tool_list: Vec<String> = tools.into_iter().map(str::to_string).collect();
    tool_list.sort();
    Ok(ToolPlan {
        v: TOOL_PLAN_VERSION,
        catalog_version: TOOL_CATALOG_VERSION,
        tools: tool_list,
        required_tools: required,
        limits: ToolPlanLimits {
            model_requests: settings.model_request_limit,
            tool_calls: settings.tool_call_limit,
            writes: settings.write_call_limit,
            input_tokens: settings.input_token_limit,
            output_tokens: settings.output_token_limit,
            total_tokens: settings.total_token_limit,
            wall_seconds: settings.execution_timeout_secs,
        },
        unavailable_capabilities: UNAVAILABLE_CAPABILITIES
            .into_iter()
            .map(str::to_string)
            .collect(),
        extra_toolsets: match creator_extra_toolsets {
            Some(check) => check(),
            None => false,
        },
    })
}

// ---------------------------------------------------------------------------
// Dispatch-time re-intersection (`policy.py:162-196`)
// ---------------------------------------------------------------------------

/// A project scope for run-project resolution (`policy.py:168-174`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectRef {
    pub project_id: Uuid,
    pub workspace_id: Uuid,
}

/// Which project a run's github state is read from (`policy.py:168-174`): the bound
/// work item's project, else the scheduler binding's, else the pod's.
pub fn resolve_run_project(
    work_item: Option<ProjectRef>,
    scheduler_binding: Option<ProjectRef>,
    pod: ProjectRef,
) -> ProjectRef {
    work_item.or(scheduler_binding).unwrap_or(pod)
}

/// The linked-PR leg of `resolve_current_tool_names` (`policy.py:177-191`).
///
/// `Some` exactly when the run has a bound work item (the caller ran
/// [`BINDING_REPO_SQL`] for the repo scope and [`CODE_REVIEW_LINK_EXISTS_SQL`] for
/// `link_exists`); `None` skips the leg, as the `elif run.work_item_id` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkedPrCtx<'a> {
    pub repo_namespace: &'a str,
    pub repo_name: &'a str,
    pub link_exists: bool,
}

/// The binding lookup of `resolve_current_tool_names` (`policy.py:180-182`):
/// the active binding's repo scope. No workspace predicate — verbatim. Django's
/// `.get()` clears ordering and reads `LIMIT 21`; `LIMIT 1` is unobservable under
/// the partial unique constraint `git_bind_uniq_project_active` (one active binding
/// per project). Only the consumed columns are projected (`namespace`, `name`); the
/// wider `select_related` fetch is unobservable. Param: `$1` project id (uuid).
///
/// A zero-row result is impossible modulo a concurrent delete between the github
/// verdict and this lookup (Python raises `DoesNotExist` there); the caller owns
/// that race.
pub const BINDING_REPO_SQL: &str = "SELECT r.namespace, r.name FROM git_repository_bindings b INNER JOIN git_repositories r ON r.id = b.repository_id WHERE b.project_id = $1 AND b.deleted_at IS NULL LIMIT 1";

/// The code-review-link leg of `resolve_current_tool_names` (`policy.py:183-190`).
/// Exact (case-sensitive) match on `namespace` / `repo_name` — verbatim, no `LOWER`.
/// Params: `$1` issue id (uuid), `$2` namespace, `$3` repo name.
pub const CODE_REVIEW_LINK_EXISTS_SQL: &str = "SELECT 1 FROM git_code_review_links WHERE issue_id = $1 AND provider = 'github' AND host_url = 'https://github.com' AND namespace = $2 AND repo_name = $3 AND deleted_at IS NULL LIMIT 1";

/// String members of a plan array entry (`tool_plan.get(key, ())`, `policy.py:164,192`).
///
/// Real rows always carry lists of strings (written by [`build_tool_plan`]); a
/// missing key reads as empty per the `.get` default, and non-string members of a
/// present-but-corrupt value are ignored rather than panicking.
fn plan_names(plan: &serde_json::Value, key: &str) -> HashSet<String> {
    plan.get(key)
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Re-intersect the immutable plan with current grants and kill switches
/// (`policy.py:162-196`).
///
/// `github_available` is the [`github_available_for_project`] verdict for
/// [`resolve_run_project`]'s project; `linked_pr` carries the bound-issue leg (see
/// [`LinkedPrCtx`]). The `extra_toolsets` plan flag is deliberately unread here: it
/// is a sibling of the name sets, never a member, so an external change can never
/// fail an unrelated run.
pub fn resolve_current_tool_names(
    tool_plan: &serde_json::Value,
    settings: &CloudAgentSettings,
    github_available: bool,
    linked_pr: Option<LinkedPrCtx<'_>>,
) -> Result<Vec<String>, RequiredToolUnavailable> {
    let mut current = plan_names(tool_plan, "tools");
    for tool in &settings.disabled_tools {
        current.remove(tool);
    }
    if !settings.writes_enabled {
        for tool in WRITE_TOOLS {
            current.remove(tool);
        }
    }
    if !github_available {
        for tool in GITHUB_TOOLS {
            current.remove(tool);
        }
    } else if let Some(ctx) = linked_pr {
        if !ctx.link_exists {
            current.remove("github_get_linked_pull_request");
        }
    }
    let required = plan_names(tool_plan, "required_tools");
    let mut missing: Vec<&str> = required
        .iter()
        .filter(|name| !current.contains(name.as_str()))
        .map(String::as_str)
        .collect();
    missing.sort_unstable();
    if !missing.is_empty() {
        return Err(RequiredToolUnavailable::new(format!(
            "Required Cloud Agent tools are no longer available: {}",
            missing.join(", ")
        )));
    }
    let mut names: Vec<String> = current.into_iter().collect();
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::error::Error;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-03-policy.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    /// Django-default settings (`settings/common.py:531-584` + `config/registry.py`).
    /// The limits half is pinned against the fixture below, so a drift in either
    /// source fails loudly instead of silently re-baselining the goldens.
    fn django_cloud_settings() -> CloudAgentSettings {
        CloudAgentSettings {
            enabled: false,
            writes_enabled: false,
            github_tools_enabled: true,
            disabled_tools: Vec::new(),
            reconcile_interval_secs: 30,
            model_request_timeout_secs: 60,
            execution_timeout_secs: 285,
            run_soft_limit_secs: 300,
            run_hard_limit_secs: 330,
            stale_grace_secs: 60,
            dispatch_lease_secs: 60,
            dispatch_backoff_secs: 10,
            dispatch_scan_interval_secs: 10,
            sweep_interval_secs: 30,
            dispatch_scan_batch: 100,
            max_queue_age_secs: 900,
            model_request_limit: 25,
            tool_call_limit: 20,
            write_call_limit: 3,
            input_token_limit: 144_000,
            output_token_limit: 16_000,
            total_token_limit: 160_000,
            max_output_tokens_per_request: 4096,
            max_queued_per_workspace: 20,
            max_running_per_workspace: 2,
            user_creation_rate_per_minute: 6,
            workspace_creation_rate_per_minute: 30,
            tool_timeout_secs: 20,
            max_tool_result_bytes: 65536,
            max_prompt_bytes: 262_144,
            max_final_result_bytes: 65536,
            max_events: 500,
            block_private_urls: true,
        }
    }

    fn django_managed_settings() -> ManagedRunnerSettings {
        ManagedRunnerSettings {
            enabled: false,
            max_per_user_project: 1,
            queued_max_age_secs: 43200,
            graceful_stop_secs: 30,
            sweep_interval_secs: 300,
            desktop_min_version: String::new(),
        }
    }

    fn managed(available: bool, reason: &str) -> ManagedAvailability {
        ManagedAvailability {
            available,
            reason_code: reason.to_string(),
        }
    }

    fn active_user() -> UserFlags {
        UserFlags {
            is_active: true,
            is_bot: false,
        }
    }

    // -- catalogs (`policy.py:11-42`) --------------------------------------

    #[test]
    fn catalogs_match_python_verbatim() {
        // Traced to `policy.py:11-42` and cross-checked by executing the real
        // module (oracle run 2026-10-01); the build_tool_plan goldens below pin
        // membership transitively.
        assert_eq!(
            READ_TOOLS.as_slice(),
            [
                "pidash_get_current_issue",
                "pidash_list_current_issue_comments",
                "pidash_list_project_states",
                "pidash_search_project_issues",
                "pidash_get_project_issue",
                "pidash_list_linked_code_reviews",
                "pidash_list_issue_relations",
                "github_get_file",
                "github_get_linked_pull_request",
            ]
        );
        assert_eq!(
            WRITE_TOOLS.as_slice(),
            [
                "pidash_add_current_issue_comment",
                "pidash_update_current_issue_workpad",
                "pidash_transition_current_issue",
                "pidash_create_project_issue",
                "pidash_relate_issues",
                "pidash_unrelate_issues",
            ]
        );
        assert_eq!(
            REPEATABLE_WRITE_TOOLS.as_slice(),
            ["pidash_relate_issues", "pidash_unrelate_issues"]
        );
        assert_eq!(
            CURRENT_ISSUE_WRITE_TOOLS.as_slice(),
            [
                "pidash_add_current_issue_comment",
                "pidash_update_current_issue_workpad",
                "pidash_transition_current_issue",
            ]
        );
        assert_eq!(
            UNAVAILABLE_CAPABILITIES.as_slice(),
            ["filesystem", "shell", "worktree"]
        );
        assert_eq!(TOOL_PLAN_VERSION, 1);
        assert_eq!(TOOL_CATALOG_VERSION, 1);
    }

    // -- kill switches + effective executor + llm gate -----------------------

    #[test]
    fn kill_switches_read_the_settings() {
        let cloud = django_cloud_settings();
        let managed = django_managed_settings();
        assert!(!cloud_agent_is_configured(&cloud));
        assert!(!managed_runner_is_enabled(&managed));
        let cloud = CloudAgentSettings {
            enabled: true,
            ..django_cloud_settings()
        };
        let managed = ManagedRunnerSettings {
            enabled: true,
            ..django_managed_settings()
        };
        assert!(cloud_agent_is_configured(&cloud));
        assert!(managed_runner_is_enabled(&managed));
    }

    #[test]
    fn effective_executor_matches_fixture() {
        let golden = &fixture()["effective_executor_for_issue"];
        assert_eq!(
            effective_executor_for_issue(Some("cloud_agent"), "local_runner"),
            golden["override_wins"].as_str().expect("golden")
        );
        assert_eq!(
            effective_executor_for_issue(None, "local_runner"),
            golden["none_inherits_project_default"]
                .as_str()
                .expect("golden")
        );
        // A missing attribute reads as None (`getattr` default): same arm.
        assert_eq!(
            effective_executor_for_issue(None, "local_runner"),
            golden["missing_attr_inherits"].as_str().expect("golden")
        );
        // `or` treats "" as missing too (oracle-verified against Django).
        assert_eq!(
            effective_executor_for_issue(Some(""), "local_runner"),
            "local_runner"
        );
    }

    #[test]
    fn user_llm_false_branches_never_consult_the_seam() {
        let golden = &fixture()["user_has_llm_config_false_branches"];
        assert_eq!(
            user_has_llm_config(None, || panic!("seam consulted for None")),
            golden["none"].as_bool().expect("golden")
        );
        let inactive = UserFlags {
            is_active: false,
            is_bot: false,
        };
        assert_eq!(
            user_has_llm_config(Some(&inactive), || panic!("seam consulted for inactive")),
            golden["inactive"].as_bool().expect("golden")
        );
        let bot = UserFlags {
            is_active: true,
            is_bot: true,
        };
        assert_eq!(
            user_has_llm_config(Some(&bot), || panic!("seam consulted for bot")),
            golden["bot"].as_bool().expect("golden")
        );
        // The True-branch delegates to the seam verbatim (CE default path).
        assert!(user_has_llm_config(Some(&active_user()), || true));
        assert!(!user_has_llm_config(Some(&active_user()), || false));
    }

    // -- resolve_executor_kind matrix ----------------------------------------

    #[test]
    fn executor_matrix_matches_fixture() {
        let cloud_on = CloudAgentSettings {
            enabled: true,
            ..django_cloud_settings()
        };
        let cloud_off = django_cloud_settings();
        let managed_off = django_managed_settings();
        let managed_on = ManagedRunnerSettings {
            enabled: true,
            ..django_managed_settings()
        };
        // requested=null inherits the default.
        assert_eq!(
            resolve_executor_kind(None, "local_runner", &cloud_on, &managed_off),
            Ok(AgentExecutorKind::LocalRunner)
        );
        // Explicit cloud request passes when configured.
        assert_eq!(
            resolve_executor_kind(Some("cloud_agent"), "local_runner", &cloud_on, &managed_off),
            Ok(AgentExecutorKind::CloudAgent)
        );
        // Managed on a disabled instance raises with the DISABLED code.
        let err = resolve_executor_kind(
            Some("managed_runner"),
            "local_runner",
            &cloud_on,
            &managed_off,
        )
        .expect_err("managed on disabled instance raises");
        assert_eq!(
            err,
            ResolveExecutorError::ManagedRunner(ManagedRunnerUnavailable::new(
                ManagedRunnerReason::DISABLED,
                "Pi Dash Agent is not enabled on this instance",
            ))
        );
        let matrix = fixture()["resolve_executor_kind_matrix"]
            .as_array()
            .expect("matrix")
            .clone();
        assert_eq!(matrix[2]["raises"], "ManagedRunnerUnavailable");
        assert_eq!(matrix[2]["code"], ManagedRunnerReason::DISABLED);
        // Unknown values raise the bare ValueError message (no code).
        let err = resolve_executor_kind(Some("bogus"), "local_runner", &cloud_on, &managed_off)
            .expect_err("unknown executor raises");
        assert_eq!(err, ResolveExecutorError::UnknownExecutor);
        assert_eq!(err.to_string(), "unknown agent executor");
        assert_eq!(matrix[3]["raises"], "ValueError");
        assert_eq!(matrix[3]["message"], "unknown agent executor");
        // Cloud on an unconfigured instance raises with its code.
        let err = resolve_executor_kind(
            Some("cloud_agent"),
            "local_runner",
            &cloud_off,
            &managed_off,
        )
        .expect_err("unconfigured cloud raises");
        assert_eq!(
            err,
            ResolveExecutorError::CloudAgent(CloudAgentUnavailable::new(
                "Pi Dash Cloud Agent is not currently available"
            ))
        );
        assert_eq!(matrix[4]["raises"], "CloudAgentUnavailable");
        assert_eq!(matrix[4]["code"], CloudAgentUnavailable::CODE);
        // The enabled sides pass through (matrix gaps, asserted directly).
        assert_eq!(
            resolve_executor_kind(
                Some("managed_runner"),
                "local_runner",
                &cloud_on,
                &managed_on
            ),
            Ok(AgentExecutorKind::ManagedRunner)
        );
        assert_eq!(
            resolve_executor_kind(
                Some("local_runner"),
                "cloud_agent",
                &cloud_off,
                &managed_off
            ),
            Ok(AgentExecutorKind::LocalRunner)
        );
    }

    #[test]
    fn executor_resolution_edge_cases_match_python() {
        let cloud_on = CloudAgentSettings {
            enabled: true,
            ..django_cloud_settings()
        };
        let managed_off = django_managed_settings();
        // `requested or default`: "" behaves as None (oracle-verified).
        assert_eq!(
            resolve_executor_kind(Some(""), "local_runner", &cloud_on, &managed_off),
            Ok(AgentExecutorKind::LocalRunner)
        );
        // An invalid project default fails the same membership check.
        assert_eq!(
            resolve_executor_kind(None, "bogus", &cloud_on, &managed_off),
            Err(ResolveExecutorError::UnknownExecutor)
        );
        // Error taxonomy: codes, messages, std::error::Error impls.
        assert_eq!(CloudAgentUnavailable::CODE, "cloud_agent_unavailable");
        assert_eq!(
            CloudCapabilityUnavailable::CODE,
            "cloud_capability_unavailable"
        );
        assert_eq!(RequiredToolUnavailable::CODE, "required_tool_unavailable");
        let cloud_err = CloudAgentUnavailable::new("m");
        assert_eq!(cloud_err.code(), CloudAgentUnavailable::CODE);
        assert_eq!(cloud_err.message(), "m");
        assert_eq!(cloud_err.to_string(), "m");
        let _: &dyn std::error::Error = &cloud_err;
        let _: &dyn std::error::Error = &CloudCapabilityUnavailable::new("m");
        let _: &dyn std::error::Error = &RequiredToolUnavailable::new("m");
        let wrapped = ResolveExecutorError::CloudAgent(cloud_err);
        assert!(wrapped.source().is_some());
        assert!(ResolveExecutorError::UnknownExecutor.source().is_none());
    }

    // -- agent_executor_options ----------------------------------------------

    #[test]
    fn executor_options_shape_matches_fixture() {
        let shape = &fixture()["agent_executor_options_shape"];
        assert_eq!(
            shape["rows"],
            json!(["cloud_agent", "local_runner", "managed_runner"])
        );
        assert_eq!(
            shape["row_keys"],
            json!(["kind", "available", "reason_code"])
        );

        let cloud_on = CloudAgentSettings {
            enabled: true,
            ..django_cloud_settings()
        };
        let rows = agent_executor_options(
            &cloud_on,
            None,
            || panic!("no user, no seam"),
            true,
            managed(true, ""),
        );
        assert_eq!(
            rows.iter().map(|r| r.kind.value()).collect::<Vec<_>>(),
            ["cloud_agent", "local_runner", "managed_runner"]
        );
        // Row keys in order (`kind`, `available`, `reason_code`): struct
        // serialization always follows field order.
        let text = serde_json::to_string(&rows[0]).expect("serializes");
        assert_eq!(
            text,
            r#"{"kind":"cloud_agent","available":true,"reason_code":""}"#
        );
    }

    #[test]
    fn executor_options_cloud_and_local_legs() {
        let shape = &fixture()["agent_executor_options_shape"];
        let cloud_on = CloudAgentSettings {
            enabled: true,
            ..django_cloud_settings()
        };
        let cloud_off = django_cloud_settings();

        // Cloud off: unavailable + code, seam never consulted.
        let rows = agent_executor_options(
            &cloud_off,
            Some(active_user()),
            || panic!("cloud off, no seam"),
            false,
            managed(false, "managed_runner_disabled"),
        );
        assert!(!rows[0].available);
        assert_eq!(rows[0].reason_code, CloudAgentUnavailable::CODE);
        assert_eq!(
            rows[0].reason_code,
            shape["cloud_reasons"]["unavailable"]
                .as_str()
                .expect("golden")
        );

        // Cloud on + user without LLM config: llm_config_missing.
        let rows = agent_executor_options(
            &cloud_on,
            Some(active_user()),
            || false,
            false,
            managed(false, "x"),
        );
        assert!(!rows[0].available);
        assert_eq!(rows[0].reason_code, LLM_CONFIG_MISSING_REASON);
        assert_eq!(
            rows[0].reason_code,
            shape["cloud_reasons"]["no_llm_config"]
                .as_str()
                .expect("golden")
        );

        // Cloud on + user with LLM config: available.
        let rows = agent_executor_options(
            &cloud_on,
            Some(active_user()),
            || true,
            false,
            managed(false, "x"),
        );
        assert!(rows[0].available);
        assert_eq!(rows[0].reason_code, "");

        // Local leg: reason iff unavailable (the query itself is SQL, below).
        let rows = agent_executor_options(
            &cloud_on,
            None,
            || panic!("no user"),
            false,
            managed(true, ""),
        );
        assert!(!rows[1].available);
        assert_eq!(rows[1].reason_code, NO_LOCAL_RUNNER_REASON);
        assert_eq!(
            rows[1].reason_code,
            shape["local_reason"].as_str().expect("golden")
        );
        let rows = agent_executor_options(
            &cloud_on,
            None,
            || panic!("no user"),
            true,
            managed(true, ""),
        );
        assert!(rows[1].available);
        assert_eq!(rows[1].reason_code, "");

        // Managed leg passes L4's verdict through verbatim.
        let rows = agent_executor_options(
            &cloud_on,
            None,
            || panic!("no user"),
            true,
            managed(false, "desktop_not_connected"),
        );
        assert!(!rows[2].available);
        assert_eq!(rows[2].reason_code, "desktop_not_connected");
        assert_eq!(
            shape["managed_reason_passthrough"],
            "managed_runner_availability(project, user) reason verbatim"
        );
    }

    #[test]
    fn local_runner_sql_excludes_desktop_bundled() {
        assert_eq!(
            LOCAL_RUNNER_EXISTS_SQL,
            "SELECT 1 FROM runner INNER JOIN pod ON pod.id = runner.pod_id WHERE pod.project_id = $1 AND runner.workspace_id = $2 AND runner.status = 'online' AND runner.provisioning <> 'desktop_bundled' LIMIT 1"
        );
        // Fixture pins the exclusion behaviorally (bundled-only ⇒ unavailable).
        assert!(
            fixture()["agent_executor_options_shape"]["local_excludes_desktop_bundled"]
                .as_str()
                .expect("golden")
                .contains("DESKTOP_BUNDLED")
        );
    }

    // -- build_tool_plan goldens ---------------------------------------------

    /// `build_tool_plan` with the fixture's default settings (cloud on, writes off,
    /// github on, nothing disabled — the `direct_*` scenarios use `project=None`).
    fn plan(
        run_kind: &str,
        has_issue: bool,
        required: &[&str],
    ) -> Result<ToolPlan, CloudCapabilityUnavailable> {
        let settings = CloudAgentSettings {
            enabled: true,
            ..django_cloud_settings()
        };
        build_tool_plan::<fn() -> bool, fn() -> bool>(
            run_kind, has_issue, required, None, None, &settings,
        )
    }

    #[test]
    fn tool_plan_direct_has_issue_matches_fixture_byte_for_byte() {
        let golden = &fixture()["build_tool_plan"]["direct_has_issue"];
        let rendered = serde_json::to_value(plan("issue", true, &[]).expect("plan")).expect("json");
        assert_eq!(rendered, *golden);
        // Key order is part of the contract: v, catalog_version, tools,
        // required_tools, limits, unavailable_capabilities, extra_toolsets.
        let text = serde_json::to_string(&plan("issue", true, &[]).expect("plan")).expect("text");
        let mut positions = [
            "\"v\":",
            "\"catalog_version\":",
            "\"tools\":",
            "\"required_tools\":",
            "\"limits\":",
            "\"unavailable_capabilities\":",
            "\"extra_toolsets\":",
        ]
        .iter()
        .map(|key| {
            text.find(key)
                .unwrap_or_else(|| panic!("key {key} present"))
        });
        let mut last = 0;
        for position in positions.by_ref() {
            assert!(position >= last, "key order in {text}");
            last = position;
        }
        // Limits sub-order: model_requests … wall_seconds.
        let limits_at = text.find("\"limits\":").expect("limits");
        let mut last = limits_at;
        for key in [
            "\"model_requests\":",
            "\"tool_calls\":",
            "\"writes\":",
            "\"input_tokens\":",
            "\"output_tokens\":",
            "\"total_tokens\":",
            "\"wall_seconds\":",
        ] {
            let position = text
                .find(key)
                .unwrap_or_else(|| panic!("key {key} present"));
            assert!(position > last, "limits order in {text}");
            last = position;
        }
    }

    #[test]
    fn tool_plan_tool_vectors_match_fixture() {
        let golden = &fixture()["build_tool_plan"];
        let no_issue = plan("issue", false, &[]).expect("plan");
        assert_eq!(
            serde_json::to_value(&no_issue.tools).expect("json"),
            golden["direct_no_issue_tools"]
        );
        let scheduler = plan("scheduler", true, &[]).expect("plan");
        assert_eq!(
            serde_json::to_value(&scheduler.tools).expect("json"),
            golden["scheduler_tools"]
        );

        let writes = CloudAgentSettings {
            enabled: true,
            writes_enabled: true,
            ..django_cloud_settings()
        };
        let direct =
            build_tool_plan::<fn() -> bool, fn() -> bool>("issue", true, &[], None, None, &writes)
                .expect("plan");
        assert_eq!(
            serde_json::to_value(&direct.tools).expect("json"),
            golden["writes_enabled_direct_tools"]
        );
        let sched = build_tool_plan::<fn() -> bool, fn() -> bool>(
            "scheduler",
            true,
            &[],
            None,
            None,
            &writes,
        )
        .expect("plan");
        assert_eq!(
            serde_json::to_value(&sched.tools).expect("json"),
            golden["writes_enabled_scheduler_tools"]
        );
    }

    #[test]
    fn tool_plan_disabled_required_and_github_gates() {
        let golden = &fixture()["build_tool_plan"];
        // A disabled tool is removed even when required tooling is otherwise fine.
        let settings = CloudAgentSettings {
            enabled: true,
            disabled_tools: vec!["pidash_search_project_issues".to_string()],
            ..django_cloud_settings()
        };
        let direct = build_tool_plan::<fn() -> bool, fn() -> bool>(
            "issue",
            true,
            &[],
            None,
            None,
            &settings,
        )
        .expect("plan");
        assert!(!direct
            .tools
            .contains(&"pidash_search_project_issues".to_string()));
        assert_eq!(golden["disabled_tool_removed"], true);

        // Missing capabilities raise with the sorted join.
        let err = plan("issue", true, &["shell_exec"]).expect_err("shell_exec missing");
        assert_eq!(err.code(), CloudCapabilityUnavailable::CODE);
        assert_eq!(
            err.to_string(),
            golden["required_capabilities_missing"]["message"]
                .as_str()
                .expect("golden")
        );
        assert_eq!(
            golden["required_capabilities_missing"]["raises"],
            "CloudCapabilityUnavailable"
        );
        // Duplicate capabilities dedup (set semantics, oracle-verified).
        let ok = plan("issue", true, &["github_get_file", "github_get_file"]).expect("plan");
        assert_eq!(
            serde_json::to_value(&ok.required_tools).expect("json"),
            golden["required_capabilities_ok"]
        );
        assert_eq!(ok.required_tools, ["github_get_file"]);

        // Project gate: unavailable strips github_*; available keeps them.
        let settings = CloudAgentSettings {
            enabled: true,
            ..django_cloud_settings()
        };
        let stripped = build_tool_plan(
            "issue",
            true,
            &[],
            Some(|| false),
            None::<fn() -> bool>,
            &settings,
        )
        .expect("plan");
        assert!(!stripped.tools.contains(&"github_get_file".to_string()));
        assert!(!stripped
            .tools
            .contains(&"github_get_linked_pull_request".to_string()));
        let kept = build_tool_plan(
            "issue",
            true,
            &[],
            Some(|| true),
            None::<fn() -> bool>,
            &settings,
        )
        .expect("plan");
        assert!(kept.tools.contains(&"github_get_file".to_string()));
        assert!(kept
            .tools
            .contains(&"github_get_linked_pull_request".to_string()));
        // Kill switch off: the project check is never consulted.
        let off = CloudAgentSettings {
            enabled: true,
            github_tools_enabled: false,
            ..django_cloud_settings()
        };
        let stripped = build_tool_plan(
            "issue",
            true,
            &[],
            Some(|| panic!("switch off, no project check")),
            None::<fn() -> bool>,
            &off,
        )
        .expect("plan");
        assert!(!stripped.tools.contains(&"github_get_file".to_string()));
        assert!(golden["note_github_project_gate"]
            .as_str()
            .expect("golden")
            .contains("policy.py:119-122"));
    }

    #[test]
    fn tool_plan_extra_toolsets_snapshot_semantics() {
        let settings = CloudAgentSettings {
            enabled: true,
            ..django_cloud_settings()
        };
        // No creator: false, no seam to consult.
        let no_creator = build_tool_plan::<fn() -> bool, fn() -> bool>(
            "issue",
            true,
            &[],
            None,
            None,
            &settings,
        )
        .expect("plan");
        assert!(!no_creator.extra_toolsets);
        // Creator: the seam verdict is snapshotted (CE seam returns false; an
        // overlay returning true records permission, not names).
        let denied = build_tool_plan(
            "issue",
            true,
            &[],
            None::<fn() -> bool>,
            Some(|| false),
            &settings,
        )
        .expect("plan");
        assert!(!denied.extra_toolsets);
        let admitted = build_tool_plan(
            "issue",
            true,
            &[],
            None::<fn() -> bool>,
            Some(|| true),
            &settings,
        )
        .expect("plan");
        assert!(admitted.extra_toolsets);
        // A sibling of `tools`, never a member: the flag changes no names.
        assert_eq!(admitted.tools, denied.tools);
        assert_eq!(admitted.required_tools, denied.required_tools);
        assert!(!admitted.tools.iter().any(|t| t.contains("extra")));
    }

    // -- github gate ----------------------------------------------------------

    #[test]
    fn github_gate_short_circuits_on_the_kill_switch() {
        let shape = &fixture()["github_available_for_project_shape"];
        assert!(shape["kill_switch"]
            .as_str()
            .expect("golden")
            .contains("without touching DB"));
        let off = CloudAgentSettings {
            github_tools_enabled: false,
            ..django_cloud_settings()
        };
        assert!(!github_available_for_project(&off, || panic!(
            "switch off, no DB"
        )));
        let on = django_cloud_settings();
        assert!(github_available_for_project(&on, || true));
        assert!(!github_available_for_project(&on, || false));
    }

    #[test]
    fn github_binding_sql_matches_filter_shape() {
        assert_eq!(
            GITHUB_BINDING_EXISTS_SQL,
            "SELECT 1 FROM git_repository_bindings b INNER JOIN git_repositories r ON r.id = b.repository_id INNER JOIN git_provider_accounts a ON a.id = b.provider_account_id INNER JOIN workspace_integrations w ON w.id = a.workspace_integration_id INNER JOIN github_app_installations i ON i.workspace_integration_id = w.id WHERE b.project_id = $1 AND b.workspace_id = $2 AND b.deleted_at IS NULL AND r.provider = 'github' AND r.host_url = 'https://github.com' AND a.workspace_id = $2 AND a.provider = 'github' AND a.host_url = 'https://github.com' AND a.auth_type = 'github_app' AND a.status = 'connected' AND a.verified_at IS NOT NULL AND i.verified_at IS NOT NULL AND i.suspended_at IS NULL LIMIT 1"
        );
        // Table names cross-checked against the ported models.
        for table in [
            pidash_db::integrations::git_models::git_repository_binding::TABLE,
            pidash_db::integrations::git_models::git_repository::TABLE,
            pidash_db::integrations::git_models::git_provider_account::TABLE,
            pidash_db::app_integrations::models_webhook::workspace_integration::TABLE,
            pidash_db::integrations::github_models::github_app_installation::TABLE,
        ] {
            assert!(GITHUB_BINDING_EXISTS_SQL.contains(table), "joins {table}");
        }
        assert!(fixture()["github_available_for_project_shape"]["filter"]
            .as_str()
            .expect("golden")
            .contains("auth_type=github_app"));
    }

    // -- resolve_current_tool_names -------------------------------------------

    fn plan_value(tools: &[&str], required: &[&str]) -> Value {
        json!({"tools": tools, "required_tools": required})
    }

    fn linked(link_exists: bool) -> Option<LinkedPrCtx<'static>> {
        Some(LinkedPrCtx {
            repo_namespace: "octo",
            repo_name: "demo",
            link_exists,
        })
    }

    #[test]
    fn current_tool_names_follow_the_fixture_rule() {
        let shape = &fixture()["resolve_current_tool_names_shape"];
        assert!(shape["rule"]
            .as_str()
            .expect("golden")
            .contains("CLOUD_AGENT_DISABLED_TOOLS"));
        assert_eq!(shape["sorted"], true);

        let settings = django_cloud_settings();
        let full = plan("issue", true, &[]).expect("plan");
        let stored = serde_json::to_value(&full).expect("json");

        // Steady state: everything survives, sorted.
        let names =
            resolve_current_tool_names(&stored, &settings, true, linked(true)).expect("names");
        assert_eq!(names, full.tools);

        // Disabled tools drop out.
        let settings = CloudAgentSettings {
            disabled_tools: vec!["pidash_search_project_issues".to_string()],
            ..django_cloud_settings()
        };
        let names =
            resolve_current_tool_names(&stored, &settings, true, linked(true)).expect("names");
        assert!(!names.contains(&"pidash_search_project_issues".to_string()));

        // Writes off drops WRITE_TOOLS (plan admitted with writes on).
        let writes = CloudAgentSettings {
            enabled: true,
            writes_enabled: true,
            ..django_cloud_settings()
        };
        let admitted =
            build_tool_plan::<fn() -> bool, fn() -> bool>("issue", true, &[], None, None, &writes)
                .expect("plan");
        let stored = serde_json::to_value(&admitted).expect("json");
        let names =
            resolve_current_tool_names(&stored, &settings, true, linked(true)).expect("names");
        for tool in WRITE_TOOLS {
            assert!(!names.contains(&tool.to_string()), "{tool} dropped");
        }

        // Github unavailable drops both github tools (the linked-PR leg is skipped).
        let names =
            resolve_current_tool_names(&stored, &settings, false, linked(true)).expect("names");
        assert!(!names.contains(&"github_get_file".to_string()));
        assert!(!names.contains(&"github_get_linked_pull_request".to_string()));

        // Github available + bound issue without a live link drops only the PR tool.
        let stored = serde_json::to_value(&full).expect("json");
        let names =
            resolve_current_tool_names(&stored, &settings, true, linked(false)).expect("names");
        assert!(names.contains(&"github_get_file".to_string()));
        assert!(!names.contains(&"github_get_linked_pull_request".to_string()));

        // Github available + no bound issue keeps both.
        let names = resolve_current_tool_names(&stored, &settings, true, None).expect("names");
        assert!(names.contains(&"github_get_file".to_string()));
        assert!(names.contains(&"github_get_linked_pull_request".to_string()));

        // Output is sorted.
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);

        // The extra_toolsets flag is unread here (sibling, never member).
        let flagged = json!({
            "tools": full.tools,
            "required_tools": [],
            "extra_toolsets": true,
        });
        let names =
            resolve_current_tool_names(&flagged, &settings, true, linked(true)).expect("names");
        assert!(!names.iter().any(|t| t.contains("extra")));
    }

    #[test]
    fn current_tool_names_raise_on_missing_required() {
        let settings = django_cloud_settings();
        // A required tool withdrawn by the writes kill switch raises.
        let stored = plan_value(
            &["pidash_search_project_issues", "pidash_relate_issues"],
            &["pidash_relate_issues"],
        );
        let err = resolve_current_tool_names(&stored, &settings, true, None).expect_err("raises");
        assert_eq!(err.code(), RequiredToolUnavailable::CODE);
        assert_eq!(
            err.to_string(),
            "Required Cloud Agent tools are no longer available: pidash_relate_issues"
        );
        // Missing names join sorted.
        let stored = plan_value(&["pidash_search_project_issues"], &["zzz_tool", "aaa_tool"]);
        let err = resolve_current_tool_names(&stored, &settings, true, None).expect_err("raises");
        assert_eq!(
            err.to_string(),
            "Required Cloud Agent tools are no longer available: aaa_tool, zzz_tool"
        );
        // Missing plan keys read as empty (the `.get` defaults).
        let empty = json!({});
        assert_eq!(
            resolve_current_tool_names(&empty, &settings, true, None).expect("names"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn current_tool_names_sql_and_project_chain() {
        assert_eq!(
            BINDING_REPO_SQL,
            "SELECT r.namespace, r.name FROM git_repository_bindings b INNER JOIN git_repositories r ON r.id = b.repository_id WHERE b.project_id = $1 AND b.deleted_at IS NULL LIMIT 1"
        );
        assert_eq!(
            CODE_REVIEW_LINK_EXISTS_SQL,
            "SELECT 1 FROM git_code_review_links WHERE issue_id = $1 AND provider = 'github' AND host_url = 'https://github.com' AND namespace = $2 AND repo_name = $3 AND deleted_at IS NULL LIMIT 1"
        );
        for table in [
            pidash_db::integrations::git_models::git_repository_binding::TABLE,
            pidash_db::integrations::git_models::git_repository::TABLE,
        ] {
            assert!(BINDING_REPO_SQL.contains(table), "projects {table}");
        }
        assert!(CODE_REVIEW_LINK_EXISTS_SQL
            .contains(pidash_db::integrations::git_models::git_code_review_link::TABLE));

        // work_item > scheduler_binding > pod (`policy.py:168-174`).
        let work_item = ProjectRef {
            project_id: Uuid::from_u128(1),
            workspace_id: Uuid::from_u128(11),
        };
        let scheduler = ProjectRef {
            project_id: Uuid::from_u128(2),
            workspace_id: Uuid::from_u128(22),
        };
        let pod = ProjectRef {
            project_id: Uuid::from_u128(3),
            workspace_id: Uuid::from_u128(33),
        };
        assert_eq!(
            resolve_run_project(Some(work_item), Some(scheduler), pod),
            work_item
        );
        assert_eq!(resolve_run_project(None, Some(scheduler), pod), scheduler);
        assert_eq!(resolve_run_project(None, None, pod), pod);
    }
}
