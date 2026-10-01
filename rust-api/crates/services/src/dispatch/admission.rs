//! Admission, desktop permission, and configuration checks: D-11 L4 (stage 5).
//!
//! Port of `apps/api/pi_dash/cloud_agent/admission.py:15-76`,
//! `apps/api/pi_dash/cloud_agent/api.py:5-8`,
//! `apps/api/pi_dash/cloud_agent/checks.py:8-74`,
//! `apps/api/pi_dash/managed_runner/policy.py:27-96`,
//! `apps/api/pi_dash/managed_runner/checks.py:20-50`, and
//! `apps/api/pi_dash/managed_runner/permissions.py:12-28`:
//!
//! * `CloudAgentAdmissionError` (`admission.py:15-19`) →
//!   [`CloudAgentAdmissionError`].
//! * `_take` / `_consume` (`admission.py:22-58`) → [`take_bucket`] (private) /
//!   [`consume_admission_token`] over the [`AdmissionCache`] seam.
//! * `enforce_creation_rate` (`admission.py:61-76`) →
//!   [`enforce_creation_rate`] + [`admission_bucket`] /
//!   [`admission_retry_after`] / [`workspace_admission_key`] /
//!   [`user_admission_key`].
//! * `CloudAgentUnavailableAPI` (`api.py:5-8`) →
//!   [`CLOUD_AGENT_UNAVAILABLE_HTTP_STATUS`] + [`CloudAgentUnavailableBody`].
//! * `cloud_agent_configuration_check` (`checks.py:8-74`) →
//!   [`cloud_agent_configuration_check`] (E001/E002/E005/E007/E008).
//! * `managed_llm_profile` / `enrolled_managed_runners` /
//!   `online_managed_runner` / `managed_runner_availability`
//!   (`managed_runner/policy.py:27-96`) → the [`LlmProfile`] seam verdict +
//!   [`ENROLLED_MANAGED_RUNNERS_EXISTS_SQL`] / [`ONLINE_MANAGED_RUNNER_SQL`] +
//!   [`managed_runner_availability`].
//! * `managed_runner_configuration_check` (`managed_runner/checks.py:20-50`)
//!   → [`managed_runner_configuration_check`] (E001/E003/E004 + the data-driven
//!   E002 recipe cross-check over [`PhaseTemplate`] rows).
//! * `IsDesktopSession` (`permissions.py:12-28`) → [`is_desktop_session`] +
//!   [`DesktopSessionRequiredBody`].
//!
//! Translation notes:
//!
//! * Settings arrive as the central structs
//!   (`pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings}`); no
//!   parallel settings struct is defined. `AgentExecutorKind` membership and
//!   `ManagedRunnerReason` codes reuse L1; `ManagedAvailability`, `UserFlags`,
//!   `READ_TOOLS` / `WRITE_TOOLS`, and the `managed_runner_is_enabled` switch
//!   reuse L3 (`super::policy`).
//! * `transaction.on_commit` has no ambient equivalent in services (Porting
//!   guide: transactions are a wrapper collecting post-commit actions), so the
//!   caller passes the deferral sink (`&mut dyn FnMut(DeferredConsume)`)
//!   explicitly; dropping the sink without running it is the rollback path,
//!   which burns no quota exactly as Python's does.
//! * EE-overlayable seams arrive as inputs, never reimplemented:
//!   `request_is_desktop` (F-10, `ee/authentication/desktop.py:25-33`) and the
//!   model profile (`managed_llm_profile`, `ee/assistant/model_provider.py:71`)
//!   arrive as an `FnOnce` closure / verdict so the Python short-circuit
//!   structure (seam consulted only when reached) is preserved and provable.
//! * `int(time.time())` arrives as `now_unix_secs: i64`; `div_euclid` /
//!   `rem_euclid` reproduce Python `//` / `%` exactly (equal to `/` / `%` for
//!   real timestamps, exact for all inputs).
//! * Denial bodies are serde structs with fields in Python dict order, not
//!   `json!` maps, so the serialized bytes keep `error` first.
//! * The managed E002 message uses `{state!r}` (single quotes); Rust `{:?}`
//!   would render double quotes, so the quotes are literal.
//!
//! Fixture: `rust-api/fixtures/dispatch/fx-disp-04-admission-checks.golden.json`
//! (FX-DISP-04), plus the `cloud_unavailable_api_shape` node of FX-DISP-03
//! (recorded by the fixture sub-issue, unpinned by L3 which deferred `api.py`
//! here).
//!
//! Ported bugs: none found in these units on read-through. The burst
//! overshoot (`admission.py:40-42`) and the +5s consume overhang (`:24`) are
//! documented-by-design and translated as-is.

use super::policy::{
    managed_runner_is_enabled, CloudAgentUnavailable, ManagedAvailability, UserFlags, READ_TOOLS,
    WRITE_TOOLS,
};
use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
use pidash_types::dispatch::{AgentExecutorKind, ManagedRunnerReason};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

// ---------------------------------------------------------------------------
// Admission error (`admission.py:15-19`)
// ---------------------------------------------------------------------------

/// Admission refusal (`admission.py:15-19`; a `RuntimeError` in Python).
///
/// Raised by [`take_bucket`] (`run_quota_exceeded` / `admission_unavailable`)
/// and, in the creation layer (L6, `creation.py:63-69,169-173`), for a full
/// workspace queue — so `detail` is stored, not derived from `code`. The
/// `from exc` chain on the cache-down raise is a traceback detail; `code` /
/// `detail` / `retry_after_seconds` are the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudAgentAdmissionError {
    code: String,
    detail: String,
    retry_after_seconds: Option<i64>,
}

impl CloudAgentAdmissionError {
    /// Bucket full (`admission.py:52-57`).
    pub const RUN_QUOTA_EXCEEDED: &'static str = "run_quota_exceeded";
    /// Cache read failed (`admission.py:46-51`).
    pub const ADMISSION_UNAVAILABLE: &'static str = "admission_unavailable";

    /// `CloudAgentAdmissionError(code, detail, retry_after_seconds=None)`
    /// (`admission.py:16-19`).
    pub fn new(
        code: impl Into<String>,
        detail: impl Into<String>,
        retry_after_seconds: Option<i64>,
    ) -> Self {
        CloudAgentAdmissionError {
            code: code.into(),
            detail: detail.into(),
            retry_after_seconds,
        }
    }

    /// The refusal code.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// `str(exc)`: the detail (`:17` passes it to `RuntimeError`).
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Seconds until the next minute bucket (always `Some` at the raise
    /// sites in this module; `None` is the constructor default).
    pub fn retry_after_seconds(&self) -> Option<i64> {
        self.retry_after_seconds
    }
}

impl std::fmt::Display for CloudAgentAdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail)
    }
}

impl std::error::Error for CloudAgentAdmissionError {}

// ---------------------------------------------------------------------------
// Bucket math and keys (`admission.py:63-76`)
// ---------------------------------------------------------------------------

/// `now // 60` (`admission.py:65`).
pub fn admission_bucket(now_unix_secs: i64) -> i64 {
    now_unix_secs.div_euclid(60)
}

/// `60 - (now % 60)` (`admission.py:64`): always `1..=60`.
pub fn admission_retry_after(now_unix_secs: i64) -> i64 {
    60 - now_unix_secs.rem_euclid(60)
}

/// `cloud-agent:admission:workspace:{id}:{bucket}` (`admission.py:67`).
pub fn workspace_admission_key(workspace_id: &str, bucket: i64) -> String {
    format!("cloud-agent:admission:workspace:{workspace_id}:{bucket}")
}

/// `cloud-agent:admission:user:{id}:{bucket}` (`admission.py:73`).
pub fn user_admission_key(actor_id: &str, bucket: i64) -> String {
    format!("cloud-agent:admission:user:{actor_id}:{bucket}")
}

/// `_consume` overhang (`admission.py:24`): `timeout=retry_after_seconds + 5`.
pub const CONSUME_TIMEOUT_OVERHANG_SECS: i64 = 5;

// ---------------------------------------------------------------------------
// Cache seam (`admission.py:22-58`)
// ---------------------------------------------------------------------------

/// The cache half of `_take` / `_consume` (`admission.py:22-58`).
///
/// `bucket_count` is `int(cache.get(key) or 0)` (`:45`): a missing key reads
/// as `None` (→ 0); a backend failure surfaces as `Err` and fails closed.
/// `add_or_incr` is `cache.add(key, 1, timeout) or cache.incr(key)` (`:24-25`).
pub trait AdmissionCache {
    /// The backend failure type (`_take` fails closed on it).
    type Error: std::error::Error;
    /// Current bucket count (`None` = key absent).
    fn bucket_count(&self, key: &str) -> Result<Option<i64>, Self::Error>;
    /// Set-if-absent with TTL, else increment.
    fn add_or_incr(&self, key: &str, timeout_secs: i64) -> Result<(), Self::Error>;
}

/// One `_consume` deferred to `on_commit` (`admission.py:58`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredConsume {
    /// The bucket key to consume.
    pub key: String,
    /// `retry_after_seconds + 5` (`admission.py:24`).
    pub timeout_secs: i64,
}

/// Best-effort post-commit consumption (`_consume`, `admission.py:22-29`).
///
/// Swallows backend failures (Python logs and continues — services has no
/// logging facade; the swallow is the behavior): the check in `take_bucket`
/// already failed closed, so a lost increment only under-counts one bucket.
pub fn consume_admission_token<C: AdmissionCache>(cache: &C, deferred: &DeferredConsume) {
    let _ = cache.add_or_incr(&deferred.key, deferred.timeout_secs);
}

/// Reject on the bucket's current count; consume only on commit (`_take`,
/// `admission.py:32-58`).
///
/// The check-then-increment window lets a concurrent burst slightly overshoot
/// the per-minute rate (`:40-42`); the workspace queue cap stays the hard
/// limit. Private: handlers enforce through [`enforce_creation_rate`].
fn take_bucket<C: AdmissionCache>(
    cache: &C,
    on_commit: &mut dyn FnMut(DeferredConsume),
    key: String,
    limit: i64,
    retry_after: i64,
) -> Result<(), CloudAgentAdmissionError> {
    let count = cache
        .bucket_count(&key)
        .map_err(|_| {
            CloudAgentAdmissionError::new(
                CloudAgentAdmissionError::ADMISSION_UNAVAILABLE,
                "Cloud Agent admission is temporarily unavailable",
                Some(retry_after),
            )
        })?
        .unwrap_or(0);
    if count >= limit {
        return Err(CloudAgentAdmissionError::new(
            CloudAgentAdmissionError::RUN_QUOTA_EXCEEDED,
            "Cloud Agent creation rate exceeded",
            Some(retry_after),
        ));
    }
    on_commit(DeferredConsume {
        key,
        timeout_secs: retry_after + CONSUME_TIMEOUT_OVERHANG_SECS,
    });
    Ok(())
}

/// Consume fixed one-minute buckets (`enforce_creation_rate`,
/// `admission.py:61-76`).
///
/// The workspace bucket is always checked; the user bucket only when
/// `!automatic && actor_id.is_some()` (`:71`). Nothing is consumed here:
/// each passing bucket appends a [`DeferredConsume`] to `on_commit`, which the
/// caller runs after commit — a rolled-back creation burns no quota (`:34-42`).
pub fn enforce_creation_rate<C: AdmissionCache>(
    cloud: &CloudAgentSettings,
    now_unix_secs: i64,
    workspace_id: &str,
    actor_id: Option<&str>,
    automatic: bool,
    cache: &C,
    on_commit: &mut dyn FnMut(DeferredConsume),
) -> Result<(), CloudAgentAdmissionError> {
    let retry_after = admission_retry_after(now_unix_secs);
    let bucket = admission_bucket(now_unix_secs);
    take_bucket(
        cache,
        on_commit,
        workspace_admission_key(workspace_id, bucket),
        cloud.workspace_creation_rate_per_minute,
        retry_after,
    )?;
    if !automatic {
        if let Some(actor) = actor_id {
            take_bucket(
                cache,
                on_commit,
                user_admission_key(actor, bucket),
                cloud.user_creation_rate_per_minute,
                retry_after,
            )?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Cloud unavailable API error (`api.py:5-8`)
// ---------------------------------------------------------------------------

/// `CloudAgentUnavailableAPI.status_code` (`api.py:6`): DRF 409.
pub const CLOUD_AGENT_UNAVAILABLE_HTTP_STATUS: u16 = 409;

/// `CloudAgentUnavailableAPI.default_detail` (`api.py:7`), fields in dict
/// order. The `code` reuses L3's [`CloudAgentUnavailable::CODE`] (`policy.py:46`,
/// the same value `api.py:8` repeats as `default_code`) so the two can never
/// drift apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudAgentUnavailableBody {
    pub error: String,
    pub code: String,
}

impl CloudAgentUnavailableBody {
    /// The default 409 body, verbatim.
    pub fn new() -> Self {
        CloudAgentUnavailableBody {
            error: "Pi Dash Cloud Agent is not currently available".to_string(),
            code: CloudAgentUnavailable::CODE.to_string(),
        }
    }
}

impl Default for CloudAgentUnavailableBody {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Managed runner availability (`managed_runner/policy.py:27-96`)
// ---------------------------------------------------------------------------

/// The `(available, reason_code)` projection of `AgentModelProfile`
/// (`ee/assistant/model_provider.py:25-42`) that `managed_runner_availability`
/// reads (`policy.py:88-90`). Produced by the `managed_llm_profile` seam
/// (`managed_runner/policy.py:27-36`, itself a delegate of
/// `agent_model_profile_for_user`), never constructed in production code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmProfile {
    /// `profile.available`.
    pub available: bool,
    /// `profile.reason_code` (empty exactly when available).
    pub reason_code: String,
}

/// `(available, reason_code)` for the managed runner on one project
/// (`managed_runner_availability`, `managed_runner/policy.py:76-96`).
///
/// Gates in fixed order, first failure wins (`:79-81`): instance switch →
/// viewer present/active/non-bot → LLM profile seam → enrolled → online.
/// `user` reuses L3's [`UserFlags`]; the verdict reuses L3's
/// [`ManagedAvailability`]. `llm_profile` is consulted only when reached;
/// `enrolled_exists` / `online_exists` are the [`ENROLLED_MANAGED_RUNNERS_EXISTS_SQL`]
/// / [`ONLINE_MANAGED_RUNNER_SQL`] verdicts.
pub fn managed_runner_availability<F>(
    managed: &ManagedRunnerSettings,
    user: Option<&UserFlags>,
    llm_profile: F,
    enrolled_exists: bool,
    online_exists: bool,
) -> ManagedAvailability
where
    F: FnOnce() -> LlmProfile,
{
    if !managed_runner_is_enabled(managed) {
        return unavailable(ManagedRunnerReason::DISABLED);
    }
    match user {
        None => return unavailable(ManagedRunnerReason::NOT_CONNECTED),
        Some(flags) if !flags.is_active || flags.is_bot => {
            return unavailable(ManagedRunnerReason::NOT_CONNECTED)
        }
        Some(_) => {}
    }
    let profile = llm_profile();
    if !profile.available {
        // `profile.reason_code or LLM_CONFIG_MISSING` (`policy.py:90`): an
        // empty reason falls back, mirroring Python's `or` on strings.
        let reason = if profile.reason_code.is_empty() {
            ManagedRunnerReason::LLM_CONFIG_MISSING
        } else {
            profile.reason_code.as_str()
        };
        return unavailable(reason);
    }
    if !enrolled_exists {
        return unavailable(ManagedRunnerReason::NO_RUNNER_FOR_PROJECT);
    }
    if !online_exists {
        return unavailable(ManagedRunnerReason::NOT_CONNECTED);
    }
    ManagedAvailability {
        available: true,
        reason_code: String::new(),
    }
}

/// `(False, reason)` (`policy.py:84-95`): every refusal carries a code, and
/// the success reason is always `""` (`:96`).
fn unavailable(reason: &str) -> ManagedAvailability {
    ManagedAvailability {
        available: false,
        reason_code: reason.to_string(),
    }
}

/// The `enrolled_managed_runners(...).exists()` leg
/// (`managed_runner/policy.py:39-55,92`): non-revoked desktop-bundled runners
/// the viewer owns on the project's pod.
///
/// Django renders `owner=user` as `owner_id = $1` and `pod__project_id` as the
/// `pod` join + `pod.project_id = $2` (same join shape as L3's
/// `LOCAL_RUNNER_EXISTS_SQL`). Table `runner` is `db_table = "runner"`
/// (`runner/models.py:479`); `pod` is `db_table = "pod"`. Params: `$1` owner
/// id (uuid), `$2` project id (uuid), `$3` workspace id (uuid).
pub const ENROLLED_MANAGED_RUNNERS_EXISTS_SQL: &str = "SELECT 1 FROM runner INNER JOIN pod ON pod.id = runner.pod_id WHERE runner.owner_id = $1 AND runner.provisioning = 'desktop_bundled' AND pod.project_id = $2 AND runner.workspace_id = $3 AND runner.revoked_at IS NULL LIMIT 1";

/// `HEARTBEAT_GRACE` (`runner/services/matcher.py:44`): a runner counts as
/// online while its last heartbeat is within 90 seconds.
pub const HEARTBEAT_GRACE_SECS: i64 = 90;

/// The `online_managed_runner(...) is None` leg
/// (`managed_runner/policy.py:58-73,94-95`): the viewer's bundled runner that
/// can take work now — enrolled, `ONLINE`, heartbeat within
/// [`HEARTBEAT_GRACE_SECS`], newest heartbeat wins
/// (`.order_by("-last_heartbeat_at").first()`, `:71-72`).
///
/// Projects `runner.id` (the stable handle + existence check): the full
/// `Runner` row shape is owned by D-13…D-15, and the only in-domain consumer
/// (availability) needs existence. `NULL` heartbeats never match (`>=` on
/// `NULL` is not true), as in Django. The caller binds `$4` to
/// `now - HEARTBEAT_GRACE_SECS`, as Python binds
/// `timezone.now() - HEARTBEAT_GRACE` (`:69`). Params: `$1` owner id (uuid),
/// `$2` project id (uuid), `$3` workspace id (uuid), `$4` heartbeat threshold
/// (timestamptz).
pub const ONLINE_MANAGED_RUNNER_SQL: &str = "SELECT runner.id FROM runner INNER JOIN pod ON pod.id = runner.pod_id WHERE runner.owner_id = $1 AND runner.provisioning = 'desktop_bundled' AND pod.project_id = $2 AND runner.workspace_id = $3 AND runner.revoked_at IS NULL AND runner.status = 'online' AND runner.last_heartbeat_at >= $4 ORDER BY runner.last_heartbeat_at DESC LIMIT 1";

// ---------------------------------------------------------------------------
// Desktop-only permission (`permissions.py:12-28`)
// ---------------------------------------------------------------------------

/// `IsDesktopSession.message` (`permissions.py:20`), fields in dict order: a
/// refusal clients can tell apart from "not signed in" (`:15-17`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopSessionRequiredBody {
    pub error: String,
    pub detail: String,
}

impl DesktopSessionRequiredBody {
    /// The denial body, verbatim.
    pub fn new() -> Self {
        DesktopSessionRequiredBody {
            error: "desktop_session_required".to_string(),
            detail: "This endpoint is available to the Pi Dash desktop app.".to_string(),
        }
    }
}

impl Default for DesktopSessionRequiredBody {
    fn default() -> Self {
        Self::new()
    }
}

/// `IsDesktopSession.has_permission` (`permissions.py:22-28`).
///
/// `authenticated` folds the three Python deny branches (missing `user` attr,
/// `user` None, `is_authenticated` false); the `request_is_desktop` EE seam
/// (F-10, `ee/authentication/desktop.py:25-33`) is consulted only for
/// authenticated callers, never reimplemented.
pub fn is_desktop_session<F>(authenticated: bool, request_is_desktop: F) -> bool
where
    F: FnOnce() -> bool,
{
    if !authenticated {
        return false;
    }
    request_is_desktop()
}

// ---------------------------------------------------------------------------
// Configuration checks (`cloud_agent/checks.py`, `managed_runner/checks.py`)
// ---------------------------------------------------------------------------

/// One Django `Error` (`id` + message) from a configuration check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigurationError {
    /// The check id (`cloud_agent.E001`, …).
    pub id: &'static str,
    /// The message, verbatim.
    pub message: String,
}

/// `cloud_agent_configuration_check` (`cloud_agent/checks.py:8-74`).
///
/// Active ids are E001/E002/E005/E007/E008; E003/E004/E006 covered retired
/// platform model settings (`checks.py:15-17`). `default_agent_executor` is
/// the `DEFAULT_AGENT_EXECUTOR` setting.
pub fn cloud_agent_configuration_check(
    cloud: &CloudAgentSettings,
    default_agent_executor: &str,
) -> Vec<ConfigurationError> {
    let mut errors = Vec::new();
    if AgentExecutorKind::from_value(default_agent_executor).is_none() {
        errors.push(ConfigurationError {
            id: "cloud_agent.E001",
            message: "DEFAULT_AGENT_EXECUTOR is invalid".to_string(),
        });
    }
    if default_agent_executor == AgentExecutorKind::CloudAgent.value() && !cloud.enabled {
        errors.push(ConfigurationError {
            id: "cloud_agent.E002",
            message: "Cloud default requires CLOUD_AGENT_ENABLED".to_string(),
        });
    }
    // `sorted(set(disabled) - set(READ) - set(WRITE))` (`checks.py:20`): sorted
    // unique unknown names; byte order in both languages.
    let known: BTreeSet<&str> = READ_TOOLS
        .iter()
        .copied()
        .chain(WRITE_TOOLS.iter().copied())
        .collect();
    let unknown: BTreeSet<&str> = cloud
        .disabled_tools
        .iter()
        .map(String::as_str)
        .filter(|tool| !known.contains(tool))
        .collect();
    if !unknown.is_empty() {
        errors.push(ConfigurationError {
            id: "cloud_agent.E005",
            message: format!(
                "CLOUD_AGENT_DISABLED_TOOLS contains unknown names: {}",
                unknown.iter().copied().collect::<Vec<_>>().join(", ")
            ),
        });
    }
    if !(0 < cloud.execution_timeout_secs
        && cloud.execution_timeout_secs < cloud.run_soft_limit_secs
        && cloud.run_soft_limit_secs < cloud.run_hard_limit_secs)
    {
        errors.push(ConfigurationError {
            id: "cloud_agent.E007",
            message: "Cloud Agent execution, soft, and hard timeouts must be positive and strictly increasing"
                .to_string(),
        });
    }
    // `positive_settings` (`checks.py:40-65`), in list order; the report keeps
    // that order (`:66` is a list comprehension, not a set).
    let positive: [(&str, i64); 24] = [
        (
            "CLOUD_AGENT_MODEL_REQUEST_TIMEOUT_SECONDS",
            cloud.model_request_timeout_secs,
        ),
        ("CLOUD_AGENT_STALE_GRACE_SECONDS", cloud.stale_grace_secs),
        (
            "CLOUD_AGENT_DISPATCH_LEASE_SECONDS",
            cloud.dispatch_lease_secs,
        ),
        (
            "CLOUD_AGENT_DISPATCH_BACKOFF_SECONDS",
            cloud.dispatch_backoff_secs,
        ),
        (
            "CLOUD_AGENT_DISPATCH_SCAN_INTERVAL_SECONDS",
            cloud.dispatch_scan_interval_secs,
        ),
        (
            "CLOUD_AGENT_SWEEP_INTERVAL_SECONDS",
            cloud.sweep_interval_secs,
        ),
        ("CLOUD_AGENT_DISPATCH_SCAN_BATCH", cloud.dispatch_scan_batch),
        (
            "CLOUD_AGENT_MAX_QUEUE_AGE_SECONDS",
            cloud.max_queue_age_secs,
        ),
        ("CLOUD_AGENT_MODEL_REQUEST_LIMIT", cloud.model_request_limit),
        ("CLOUD_AGENT_TOOL_CALL_LIMIT", cloud.tool_call_limit),
        ("CLOUD_AGENT_WRITE_CALL_LIMIT", cloud.write_call_limit),
        ("CLOUD_AGENT_INPUT_TOKEN_LIMIT", cloud.input_token_limit),
        ("CLOUD_AGENT_OUTPUT_TOKEN_LIMIT", cloud.output_token_limit),
        ("CLOUD_AGENT_TOTAL_TOKEN_LIMIT", cloud.total_token_limit),
        (
            "CLOUD_AGENT_MAX_OUTPUT_TOKENS_PER_REQUEST",
            cloud.max_output_tokens_per_request,
        ),
        (
            "CLOUD_AGENT_MAX_QUEUED_PER_WORKSPACE",
            cloud.max_queued_per_workspace,
        ),
        (
            "CLOUD_AGENT_MAX_RUNNING_PER_WORKSPACE",
            cloud.max_running_per_workspace,
        ),
        (
            "CLOUD_AGENT_USER_CREATION_RATE_PER_MINUTE",
            cloud.user_creation_rate_per_minute,
        ),
        (
            "CLOUD_AGENT_WORKSPACE_CREATION_RATE_PER_MINUTE",
            cloud.workspace_creation_rate_per_minute,
        ),
        ("CLOUD_AGENT_TOOL_TIMEOUT_SECONDS", cloud.tool_timeout_secs),
        (
            "CLOUD_AGENT_MAX_TOOL_RESULT_BYTES",
            cloud.max_tool_result_bytes,
        ),
        ("CLOUD_AGENT_MAX_PROMPT_BYTES", cloud.max_prompt_bytes),
        (
            "CLOUD_AGENT_MAX_FINAL_RESULT_BYTES",
            cloud.max_final_result_bytes,
        ),
        ("CLOUD_AGENT_MAX_EVENTS", cloud.max_events),
    ];
    let invalid: Vec<&str> = positive
        .iter()
        .filter(|(_, value)| *value <= 0)
        .map(|(name, _)| *name)
        .collect();
    if !invalid.is_empty() {
        errors.push(ConfigurationError {
            id: "cloud_agent.E008",
            message: format!(
                "Cloud Agent limits must be positive: {}",
                invalid.join(", ")
            ),
        });
    }
    errors
}

/// One `PHASES` row's contribution to managed E002
/// (`orchestration/agent_phases.py:75-83`): only `state_name` and
/// `template_name` drive the cross-check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseTemplate<'a> {
    /// `cfg.state_name` (used in the E002 message).
    pub state_name: &'a str,
    /// `cfg.template_name` (looked up in the recipe table).
    pub template_name: &'a str,
}

/// `managed_runner_configuration_check` (`managed_runner/checks.py:20-50`).
///
/// E001/E003/E004 plus the orchestration-`PHASES` × `MANAGED_RECIPES`
/// cross-check as a data-driven rule (`checks.py:42-49`): one E002 per phase
/// (in phase order) whose `template_name` has no recipe-table entry. Both
/// tables arrive as data — `managed_recipes` is shaped like
/// `crate::prompting::recipes::MANAGED_RECIPES` so the live table plugs in
/// directly.
pub fn managed_runner_configuration_check(
    managed: &ManagedRunnerSettings,
    default_agent_executor: &str,
    phases: &[PhaseTemplate<'_>],
    managed_recipes: &[(&str, &[&str])],
) -> Vec<ConfigurationError> {
    let mut errors = Vec::new();
    if default_agent_executor == AgentExecutorKind::ManagedRunner.value() && !managed.enabled {
        errors.push(ConfigurationError {
            id: "managed.E001",
            message: "DEFAULT_AGENT_EXECUTOR=managed_runner requires MANAGED_RUNNER_ENABLED"
                .to_string(),
        });
    }
    if managed.max_per_user_project < 1 {
        errors.push(ConfigurationError {
            id: "managed.E003",
            message: "MANAGED_RUNNER_MAX_PER_USER_PROJECT must be at least 1".to_string(),
        });
    }
    if managed.queued_max_age_secs <= 0 {
        errors.push(ConfigurationError {
            id: "managed.E004",
            message: "MANAGED_RUNNER_QUEUED_MAX_AGE_SECS must be positive".to_string(),
        });
    }
    for phase in phases {
        if !managed_recipes
            .iter()
            .any(|(kind, _)| *kind == phase.template_name)
        {
            errors.push(ConfigurationError {
                id: "managed.E002",
                // `{state!r}` renders single quotes; Rust `{:?}` would not.
                message: format!(
                    "phase '{}' has no managed-runner recipe for '{}'",
                    phase.state_name, phase.template_name
                ),
            });
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-04-admission-checks.golden.json");
    static FIXTURE_03: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-03-policy.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn fixture_03() -> Value {
        serde_json::from_str(FIXTURE_03).expect("fixture parses")
    }

    /// Django-default settings (`settings/common.py:531-584` + `config/registry.py`),
    /// same source as L3's test helpers.
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

    /// Django's `PHASES` values (`orchestration/agent_phases.py:117-147`).
    fn django_phases() -> [PhaseTemplate<'static>; 3] {
        [
            PhaseTemplate {
                state_name: "In Progress",
                template_name: "coding-task",
            },
            PhaseTemplate {
                state_name: "In Review",
                template_name: "review",
            },
            PhaseTemplate {
                state_name: "In Test",
                template_name: "test",
            },
        ]
    }

    #[derive(Debug)]
    struct FakeCacheError;

    impl std::fmt::Display for FakeCacheError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "cache down")
        }
    }

    impl std::error::Error for FakeCacheError {}

    /// In-memory [`AdmissionCache`] with injectable backend failures.
    struct FakeCache {
        counts: RefCell<HashMap<String, i64>>,
        fail_get: bool,
        fail_add: bool,
    }

    impl FakeCache {
        fn new() -> Self {
            FakeCache {
                counts: RefCell::new(HashMap::new()),
                fail_get: false,
                fail_add: false,
            }
        }

        fn preset(key: &str, count: i64) -> Self {
            let cache = Self::new();
            cache.counts.borrow_mut().insert(key.to_string(), count);
            cache
        }

        fn count(&self, key: &str) -> i64 {
            self.counts.borrow().get(key).copied().unwrap_or(0)
        }
    }

    impl AdmissionCache for FakeCache {
        type Error = FakeCacheError;

        fn bucket_count(&self, key: &str) -> Result<Option<i64>, FakeCacheError> {
            if self.fail_get {
                return Err(FakeCacheError);
            }
            Ok(self.counts.borrow().get(key).copied())
        }

        fn add_or_incr(&self, key: &str, _timeout_secs: i64) -> Result<(), FakeCacheError> {
            if self.fail_add {
                return Err(FakeCacheError);
            }
            *self.counts.borrow_mut().entry(key.to_string()).or_insert(0) += 1;
            Ok(())
        }
    }

    /// The fixture's live time example: `bucket=29845934, retry=23` implies
    /// `now = 29845934*60 + (60-23) = 1790756077`.
    const FIXTURE_NOW: i64 = 1790756077;

    fn active_user() -> UserFlags {
        UserFlags {
            is_active: true,
            is_bot: false,
        }
    }

    // -- error shape (`admission.py:15-19`) ---------------------------------

    #[test]
    fn admission_error_shape_matches_fixture() {
        let shape = &fixture()["cloud_admission_error_shape"];
        assert_eq!(shape["class"], "RuntimeError subclass");
        // Rust errors implement std::error::Error.
        let err = CloudAgentAdmissionError::new("run_quota_exceeded", "detail", Some(23));
        let _: &dyn std::error::Error = &err;
        assert_eq!(
            shape["codes"][0],
            CloudAgentAdmissionError::RUN_QUOTA_EXCEEDED
        );
        assert_eq!(
            shape["codes"][1],
            CloudAgentAdmissionError::ADMISSION_UNAVAILABLE
        );
        assert_eq!(shape["codes"].as_array().expect("codes").len(), 2);
        // str(exc) is the detail; retry passes through; None is the default.
        assert_eq!(err.to_string(), "detail");
        assert_eq!(err.code(), "run_quota_exceeded");
        assert_eq!(err.retry_after_seconds(), Some(23));
        let defaulted = CloudAgentAdmissionError::new("code", "detail", None);
        assert_eq!(defaulted.retry_after_seconds(), None);
    }

    // -- bucket math and keys (`admission.py:63-76`) --------------------------

    #[test]
    fn bucket_math_matches_fixture_example() {
        let shapes = &fixture()["admission_key_shapes"];
        assert_eq!(admission_bucket(FIXTURE_NOW), 29845934);
        assert_eq!(admission_retry_after(FIXTURE_NOW), 23);
        // Minute boundary: `now % 60 == 0` retries the full 60s.
        assert_eq!(admission_bucket(1790756040), 29845934);
        assert_eq!(admission_retry_after(1790756040), 60);
        assert_eq!(shapes["bucket"], 29845934);
        assert_eq!(shapes["retry_after_seconds"], 23);
        assert_eq!(
            workspace_admission_key("ws-1", 29845934),
            shapes["workspace_key"]
                .as_str()
                .expect("key")
                .replace("<workspace_id>", "ws-1")
        );
        assert_eq!(
            user_admission_key("actor-1", 29845934),
            shapes["user_key"]
                .as_str()
                .expect("key")
                .replace("<actor_id>", "actor-1")
        );
    }

    // -- enforce_creation_rate (`admission.py:61-76`) -------------------------

    #[test]
    fn allow_manual_defers_both_buckets() {
        assert_eq!(fixture()["admission_allow_manual"], "allowed (no raise)");
        let cloud = django_cloud_settings();
        let cache = FakeCache::new();
        let mut deferred = Vec::new();
        let result = enforce_creation_rate(
            &cloud,
            FIXTURE_NOW,
            "ws-1",
            Some("actor-1"),
            false,
            &cache,
            &mut |consume| deferred.push(consume),
        );
        assert!(result.is_ok());
        assert_eq!(
            deferred,
            vec![
                DeferredConsume {
                    key: workspace_admission_key("ws-1", 29845934),
                    timeout_secs: 28,
                },
                DeferredConsume {
                    key: user_admission_key("actor-1", 29845934),
                    timeout_secs: 28,
                },
            ]
        );
        // Nothing consumed before commit; post-commit consumes land once each.
        assert_eq!(cache.count(&deferred[0].key), 0);
        for consume in &deferred {
            consume_admission_token(&cache, consume);
        }
        assert_eq!(cache.count(&deferred[0].key), 1);
        assert_eq!(cache.count(&deferred[1].key), 1);
    }

    #[test]
    fn automatic_skips_full_user_bucket() {
        assert_eq!(
            fixture()["admission_automatic_skips_user_bucket"],
            "allowed despite full user bucket"
        );
        let cloud = django_cloud_settings();
        let user_key = user_admission_key("actor-1", 29845934);
        let cache = FakeCache::preset(&user_key, 6);
        let mut deferred = Vec::new();
        let result = enforce_creation_rate(
            &cloud,
            FIXTURE_NOW,
            "ws-1",
            Some("actor-1"),
            true,
            &cache,
            &mut |consume| deferred.push(consume),
        );
        assert!(result.is_ok());
        assert_eq!(deferred.len(), 1);
        assert_eq!(deferred[0].key, workspace_admission_key("ws-1", 29845934));
    }

    #[test]
    fn deny_branches_match_fixture() {
        let cloud = django_cloud_settings();
        // Full user bucket, manual call.
        let user_key = user_admission_key("actor-1", 29845934);
        let cache = FakeCache::preset(&user_key, 6);
        let mut deferred = Vec::new();
        let err = enforce_creation_rate(
            &cloud,
            FIXTURE_NOW,
            "ws-1",
            Some("actor-1"),
            false,
            &cache,
            &mut |consume| deferred.push(consume),
        )
        .expect_err("user bucket full");
        let golden = &fixture()["admission_deny_user_bucket"];
        assert_eq!(err.code(), golden["code"].as_str().expect("code"));
        assert_eq!(err.to_string(), golden["detail"].as_str().expect("detail"));
        assert_eq!(
            err.retry_after_seconds(),
            Some(golden["retry_after_seconds"].as_i64().expect("retry"))
        );
        // The workspace deferral was already appended (Python calls on_commit
        // for the passing bucket first); the caller runs deferred consumes
        // only on Ok + commit, so the denial burns no quota.
        assert_eq!(deferred.len(), 1);
        drop(deferred);
        assert_eq!(cache.count(&workspace_admission_key("ws-1", 29845934)), 0);
        assert_eq!(cache.count(&user_key), 6);

        // Full workspace bucket denies before the user bucket is even read.
        let ws_key = workspace_admission_key("ws-1", 29845934);
        let cache = FakeCache::preset(&ws_key, 30);
        let mut deferred = Vec::new();
        let err = enforce_creation_rate(
            &cloud,
            FIXTURE_NOW,
            "ws-1",
            Some("actor-1"),
            false,
            &cache,
            &mut |consume| deferred.push(consume),
        )
        .expect_err("workspace bucket full");
        let golden = &fixture()["admission_deny_workspace_bucket"];
        assert_eq!(err.code(), golden["code"].as_str().expect("code"));
        assert_eq!(err.to_string(), golden["detail"].as_str().expect("detail"));
        assert_eq!(
            err.retry_after_seconds(),
            Some(golden["retry_after_seconds"].as_i64().expect("retry"))
        );
        assert!(deferred.is_empty());
    }

    #[test]
    fn cache_down_fails_closed() {
        let golden = &fixture()["admission_cache_down"];
        let cloud = django_cloud_settings();
        let cache = FakeCache {
            fail_get: true,
            ..FakeCache::new()
        };
        let mut deferred = Vec::new();
        let err = enforce_creation_rate(
            &cloud,
            FIXTURE_NOW,
            "ws-1",
            Some("actor-1"),
            false,
            &cache,
            &mut |consume| deferred.push(consume),
        )
        .expect_err("cache down");
        assert_eq!(err.code(), golden["code"].as_str().expect("code"));
        assert_eq!(err.to_string(), golden["detail"].as_str().expect("detail"));
        // The fixture records code + detail; the source passes the live retry
        // too (`admission.py:47-51`).
        assert_eq!(err.retry_after_seconds(), Some(23));
        assert!(deferred.is_empty());
    }

    #[test]
    fn consume_is_best_effort_and_rollback_burns_nothing() {
        // `_consume` swallows backend failures (`admission.py:26-29`).
        let cache = FakeCache {
            fail_add: true,
            ..FakeCache::new()
        };
        consume_admission_token(
            &cache,
            &DeferredConsume {
                key: "k".to_string(),
                timeout_secs: 28,
            },
        );
        // A passed check whose creation rolls back drops its deferred consumes
        // (`admission.py:34-42`): rejections must not lock the user out.
        let cloud = django_cloud_settings();
        let cache = FakeCache::new();
        let mut deferred = Vec::new();
        enforce_creation_rate(
            &cloud,
            FIXTURE_NOW,
            "ws-1",
            Some("actor-1"),
            false,
            &cache,
            &mut |consume| deferred.push(consume),
        )
        .expect("allowed");
        drop(deferred); // rollback: post-commit wrapper never runs
        assert_eq!(cache.count(&workspace_admission_key("ws-1", 29845934)), 0);
        assert_eq!(cache.count(&user_admission_key("actor-1", 29845934)), 0);
    }

    // -- CloudAgentUnavailableAPI (`api.py:5-8`) -------------------------------

    #[test]
    fn unavailable_api_shape_matches_fx_disp_03() {
        let golden = &fixture_03()["cloud_unavailable_api_shape"];
        assert_eq!(CLOUD_AGENT_UNAVAILABLE_HTTP_STATUS, 409);
        assert_eq!(golden["status"], 409);
        let body = CloudAgentUnavailableBody::new();
        assert_eq!(body, CloudAgentUnavailableBody::default());
        assert_eq!(body.code, golden["code"].as_str().expect("code"));
        // Byte-exact body: `error` first, as DRF renders the dict.
        assert_eq!(
            serde_json::to_string(&body).expect("json"),
            "{\"error\":\"Pi Dash Cloud Agent is not currently available\",\"code\":\"cloud_agent_unavailable\"}"
        );
        assert_eq!(
            serde_json::to_value(&body).expect("value"),
            golden["detail"]
        );
    }

    // -- IsDesktopSession (`permissions.py:12-28`) -----------------------------

    #[test]
    fn desktop_denial_body_matches_fixture_byte_for_byte() {
        let golden = &fixture()["is_desktop_session"]["denial_body"];
        let body = DesktopSessionRequiredBody::new();
        assert_eq!(body, DesktopSessionRequiredBody::default());
        assert_eq!(
            serde_json::to_string(&body).expect("json"),
            "{\"error\":\"desktop_session_required\",\"detail\":\"This endpoint is available to the Pi Dash desktop app.\"}"
        );
        assert_eq!(serde_json::to_value(&body).expect("value"), *golden);
    }

    #[test]
    fn desktop_permission_branches() {
        let golden = &fixture()["is_desktop_session"];
        assert_eq!(golden["deny_unauthenticated"], false);
        assert_eq!(golden["deny_no_user"], false);
        assert_eq!(golden["deny_missing_user_attr"], false);
        // All three Python deny branches fold to unauthenticated, and the seam
        // is not consulted.
        let consulted = Cell::new(false);
        assert!(!is_desktop_session(false, || {
            consulted.set(true);
            true
        }));
        assert!(!consulted.get());
        // Authenticated delegates to the EE seam verdict as-is.
        assert!(is_desktop_session(true, || true));
        assert!(!is_desktop_session(true, || false));
    }

    // -- cloud_agent_configuration_check (`checks.py:8-74`) --------------------

    fn check_pairs(errors: &[ConfigurationError]) -> Vec<(&str, &str)> {
        errors
            .iter()
            .map(|error| (error.id, error.message.as_str()))
            .collect()
    }

    #[test]
    fn cloud_check_clean_on_django_defaults() {
        assert_eq!(
            fixture()["cloud_agent_configuration_check"]["clean"],
            serde_json::json!([])
        );
        let errors = cloud_agent_configuration_check(&django_cloud_settings(), "local_runner");
        assert!(errors.is_empty());
    }

    #[test]
    fn cloud_check_vectors_match_fixture() {
        let golden = &fixture()["cloud_agent_configuration_check"];
        // E001: invalid default.
        let errors = cloud_agent_configuration_check(&django_cloud_settings(), "bogus");
        assert_eq!(
            check_pairs(&errors),
            vec![(
                golden["E001"][0][0].as_str().expect("id"),
                golden["E001"][0][1].as_str().expect("msg"),
            )]
        );
        // E002: cloud default with the kill switch off.
        let errors = cloud_agent_configuration_check(&django_cloud_settings(), "cloud_agent");
        assert_eq!(
            check_pairs(&errors),
            vec![(
                golden["E002"][0][0].as_str().expect("id"),
                golden["E002"][0][1].as_str().expect("msg"),
            )]
        );
        // E005: unknown disabled tool.
        let mut cloud = django_cloud_settings();
        cloud.disabled_tools = vec!["nope_tool".to_string()];
        let errors = cloud_agent_configuration_check(&cloud, "local_runner");
        assert_eq!(
            check_pairs(&errors),
            vec![(
                golden["E005"][0][0].as_str().expect("id"),
                golden["E005"][0][1].as_str().expect("msg"),
            )]
        );
        // E007: flat timeout chain.
        let mut cloud = django_cloud_settings();
        cloud.run_soft_limit_secs = cloud.run_hard_limit_secs;
        let errors = cloud_agent_configuration_check(&cloud, "local_runner");
        assert_eq!(
            check_pairs(&errors),
            vec![(
                golden["E007"][0][0].as_str().expect("id"),
                golden["E007"][0][1].as_str().expect("msg"),
            )]
        );
        // E008: one non-positive limit.
        let mut cloud = django_cloud_settings();
        cloud.tool_call_limit = 0;
        let errors = cloud_agent_configuration_check(&cloud, "local_runner");
        assert_eq!(
            check_pairs(&errors),
            vec![(
                golden["E008"][0][0].as_str().expect("id"),
                golden["E008"][0][1].as_str().expect("msg"),
            )]
        );
    }

    #[test]
    fn cloud_check_e005_dedupes_sorts_and_keeps_known() {
        // Set semantics beyond the fixture: duplicates collapse, known tools
        // are excluded, output is byte-sorted.
        let mut cloud = django_cloud_settings();
        cloud.disabled_tools = vec![
            "zzz_tool".to_string(),
            "pidash_add_current_issue_comment".to_string(),
            "nope_tool".to_string(),
            "zzz_tool".to_string(),
            "github_get_file".to_string(),
        ];
        let errors = cloud_agent_configuration_check(&cloud, "local_runner");
        assert_eq!(
            check_pairs(&errors),
            vec![(
                "cloud_agent.E005",
                "CLOUD_AGENT_DISABLED_TOOLS contains unknown names: nope_tool, zzz_tool",
            )]
        );
    }

    #[test]
    fn cloud_check_e008_keeps_settings_list_order() {
        // List order beyond the fixture: the report follows `positive_settings`
        // order, not the order values were zeroed in.
        let mut cloud = django_cloud_settings();
        cloud.max_events = 0;
        cloud.model_request_timeout_secs = -1;
        let errors = cloud_agent_configuration_check(&cloud, "local_runner");
        assert_eq!(
            check_pairs(&errors),
            vec![(
                "cloud_agent.E008",
                "Cloud Agent limits must be positive: CLOUD_AGENT_MODEL_REQUEST_TIMEOUT_SECONDS, CLOUD_AGENT_MAX_EVENTS",
            )]
        );
    }

    // -- managed_runner_configuration_check (`checks.py:20-50`) ----------------

    #[test]
    fn managed_check_vectors_match_fixture() {
        let golden = &fixture()["managed_runner_configuration_check"];
        let phases = django_phases();
        let recipes: &[(&str, &[&str])] = &[
            ("coding-task", &[]),
            ("review", &[]),
            ("test", &[]),
            ("scheduler", &[]),
            ("direct", &[]),
        ];
        // E001: managed default with the kill switch off (verbatim in fixture).
        let errors = managed_runner_configuration_check(
            &django_managed_settings(),
            "managed_runner",
            &phases,
            recipes,
        );
        assert_eq!(
            check_pairs(&errors),
            vec![("managed.E001", golden["E001"].as_str().expect("msg"),)]
        );
        // E003/E004: the fixture entries gloss the trigger ("(< 1)", "(<= 0)");
        // — the messages below are verbatim from `checks.py:30-37`.
        assert_eq!(
            golden["E003"],
            "MANAGED_RUNNER_MAX_PER_USER_PROJECT must be at least 1 (< 1)"
        );
        let mut managed = django_managed_settings();
        managed.max_per_user_project = 0;
        let errors = managed_runner_configuration_check(&managed, "local_runner", &phases, recipes);
        assert_eq!(
            check_pairs(&errors),
            vec![(
                "managed.E003",
                "MANAGED_RUNNER_MAX_PER_USER_PROJECT must be at least 1",
            )]
        );
        assert_eq!(
            golden["E004"],
            "MANAGED_RUNNER_QUEUED_MAX_AGE_SECS must be positive (<= 0)"
        );
        let mut managed = django_managed_settings();
        managed.queued_max_age_secs = 0;
        let errors = managed_runner_configuration_check(&managed, "local_runner", &phases, recipes);
        assert_eq!(
            check_pairs(&errors),
            vec![(
                "managed.E004",
                "MANAGED_RUNNER_QUEUED_MAX_AGE_SECS must be positive",
            )]
        );
    }

    #[test]
    fn managed_check_e002_is_data_driven_and_in_phase_order() {
        // One error per phase whose template is missing from the recipe table
        // (fixture: "one per orchestration phase whose template_name is missing
        // from prompting.recipes.MANAGED_RECIPES"), in phase order, with
        // Python `{!r}` single quotes.
        let phases = [
            PhaseTemplate {
                state_name: "In Progress",
                template_name: "coding-task",
            },
            PhaseTemplate {
                state_name: "In Review",
                template_name: "review",
            },
            PhaseTemplate {
                state_name: "In Test",
                template_name: "test",
            },
        ];
        let recipes: &[(&str, &[&str])] = &[("review", &["review-intro"])];
        let errors = managed_runner_configuration_check(
            &django_managed_settings(),
            "local_runner",
            &phases,
            recipes,
        );
        assert_eq!(
            check_pairs(&errors),
            vec![
                (
                    "managed.E002",
                    "phase 'In Progress' has no managed-runner recipe for 'coding-task'",
                ),
                (
                    "managed.E002",
                    "phase 'In Test' has no managed-runner recipe for 'test'",
                ),
            ]
        );
    }

    #[test]
    fn managed_check_clean_with_live_recipe_table() {
        // Django's phases against the real ported MANAGED_RECIPES: no E002.
        let mut managed = django_managed_settings();
        managed.enabled = true;
        let errors = managed_runner_configuration_check(
            &managed,
            "managed_runner",
            &django_phases(),
            crate::prompting::recipes::MANAGED_RECIPES,
        );
        assert!(errors.is_empty());
    }

    // -- managed_runner_availability (`policy.py:76-96`) -----------------------

    fn profile(available: bool, reason: &str) -> LlmProfile {
        LlmProfile {
            available,
            reason_code: reason.to_string(),
        }
    }

    #[test]
    fn availability_gate_order_and_reasons() {
        let user = active_user();
        // Gate 1: instance switch off (seam never consulted).
        let consulted = Cell::new(false);
        let verdict = managed_runner_availability(
            &django_managed_settings(),
            Some(&user),
            || {
                consulted.set(true);
                profile(true, "")
            },
            true,
            true,
        );
        assert!(!verdict.available);
        assert_eq!(verdict.reason_code, ManagedRunnerReason::DISABLED);
        assert!(!consulted.get());

        let mut managed = django_managed_settings();
        managed.enabled = true;
        // Gate 2: no viewer / inactive / bot, all NOT_CONNECTED.
        for viewer in [
            None,
            Some(&UserFlags {
                is_active: false,
                is_bot: false,
            }),
            Some(&UserFlags {
                is_active: true,
                is_bot: true,
            }),
        ] {
            let verdict =
                managed_runner_availability(&managed, viewer, || profile(true, ""), true, true);
            assert!(!verdict.available);
            assert_eq!(verdict.reason_code, ManagedRunnerReason::NOT_CONNECTED);
        }
        // Gate 3: profile refusal wins over offline-ness (first failure wins).
        let verdict = managed_runner_availability(
            &managed,
            Some(&user),
            || profile(false, ManagedRunnerReason::BYOK_UNSUPPORTED),
            false,
            false,
        );
        assert!(!verdict.available);
        assert_eq!(verdict.reason_code, ManagedRunnerReason::BYOK_UNSUPPORTED);
        // Gate 3b: empty profile reason falls back to llm_config_missing.
        let verdict =
            managed_runner_availability(&managed, Some(&user), || profile(false, ""), true, true);
        assert!(!verdict.available);
        assert_eq!(verdict.reason_code, ManagedRunnerReason::LLM_CONFIG_MISSING);
        // Gate 4: enrolled but offline.
        let verdict =
            managed_runner_availability(&managed, Some(&user), || profile(true, ""), false, false);
        assert!(!verdict.available);
        assert_eq!(
            verdict.reason_code,
            ManagedRunnerReason::NO_RUNNER_FOR_PROJECT
        );
        // Gate 5: enrolled, none online.
        let verdict =
            managed_runner_availability(&managed, Some(&user), || profile(true, ""), true, false);
        assert!(!verdict.available);
        assert_eq!(verdict.reason_code, ManagedRunnerReason::NOT_CONNECTED);
        // Pass: (True, "").
        let verdict =
            managed_runner_availability(&managed, Some(&user), || profile(true, ""), true, true);
        assert_eq!(
            verdict,
            ManagedAvailability {
                available: true,
                reason_code: String::new(),
            }
        );
    }

    // -- runner SQL (`policy.py:39-73`) ----------------------------------------

    #[test]
    fn runner_sql_matches_django_filters() {
        assert_eq!(
            ENROLLED_MANAGED_RUNNERS_EXISTS_SQL,
            "SELECT 1 FROM runner INNER JOIN pod ON pod.id = runner.pod_id WHERE runner.owner_id = $1 AND runner.provisioning = 'desktop_bundled' AND pod.project_id = $2 AND runner.workspace_id = $3 AND runner.revoked_at IS NULL LIMIT 1"
        );
        assert_eq!(HEARTBEAT_GRACE_SECS, 90);
        assert_eq!(
            ONLINE_MANAGED_RUNNER_SQL,
            "SELECT runner.id FROM runner INNER JOIN pod ON pod.id = runner.pod_id WHERE runner.owner_id = $1 AND runner.provisioning = 'desktop_bundled' AND pod.project_id = $2 AND runner.workspace_id = $3 AND runner.revoked_at IS NULL AND runner.status = 'online' AND runner.last_heartbeat_at >= $4 ORDER BY runner.last_heartbeat_at DESC LIMIT 1"
        );
    }
}
