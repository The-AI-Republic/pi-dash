#![forbid(unsafe_code)]

//! Executor policy: cloud_agent + managed_runner L3 (D-11, stage 5).
//!
//! Port of the policy half of `apps/api/pi_dash/core/agent_execution.py:34-128`
//! and all of `apps/api/pi_dash/cloud_agent/policy.py:11-196`:
//!
//! * `cloud_agent_is_configured` (`agent_execution.py:34-43`) → [`cloud_agent_is_configured`].
//! * `pidash_agent_is_enabled` (`agent_execution.py:46-54`) → [`pidash_agent_is_enabled`].
//! * `effective_executor_for_issue` (`agent_execution.py:57-66`) → [`effective_executor_for_issue`].
//! * `user_has_llm_config` (`agent_execution.py:69-80`) → [`user_has_llm_config`].
//! * `agent_executor_options` (`agent_execution.py:83-128`) → [`agent_executor_options`]
//!   + [`LOCAL_RUNNER_EXISTS_SQL`].
//! * `CloudAgentUnavailable` (`policy.py:45-46`) → [`CloudAgentUnavailable`].
//! * `resolve_executor_kind` (`policy.py:57-72`) → [`resolve_executor_kind`].
//!
//! P3 step B deleted the cloud tool-catalog half (`READ_TOOLS` /
//! `WRITE_TOOLS` / `CloudCapabilityUnavailable` / `RequiredToolUnavailable` /
//! `build_tool_plan` / `resolve_current_tool_names` / GitHub-binding checks)
//! with the retired execute plane.
//!
//! Out of scope here (owned by siblings): `AgentExecutorKind` / `MACHINE_EXECUTORS` /
//! `get_default_agent_executor` (L1, `pidash_types::dispatch`); `PidashAgentReason` /
//! `PidashAgentUnavailable` (L1, reused, not redefined);
//! `CloudAgentUnavailableAPI` (`cloud_agent/api.py`, L4 PIDASHCONV-485);
//! `pidash_agent_availability` (`managed_runner/policy.py`, L4 — arrives here as a
//! precomputed [`PidashAgentAvailability`]); the `extra_toolsets_enabled_for` CE default
//! (`ee/cloud_agent/toolsets.py`, L5 PIDASHCONV-486 — arrives as a seam verdict).
//!
//! Translation notes:
//!
//! * Django settings arrive as the already-ported central structs
//!   (`pidash_db::config::{CloudAgentSettings, PidashAgentSettings}`); no
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

use pidash_db::config::PidashAgentSettings;
use pidash_db::entity::{dev_machine, pod, runner};
use pidash_types::issuing::AgentExecutorKind;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

// B catalogs deleted in P3 step B (retired tool lists).

// ---------------------------------------------------------------------------
// Kill switches (`agent_execution.py:34-54`)
// ---------------------------------------------------------------------------

/// Operator kill switch for the desktop-bundled managed runner
/// (`agent_execution.py:46-54`).
pub fn pidash_agent_is_enabled(managed: &PidashAgentSettings) -> bool {
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

/// The `resolve_executor_kind` failure mode (`policy.py:57-72`, #2903).
///
/// P1 (agent naming convention): `CloudAgent` is unreachable — a stored
/// `cloud_agent` value resolves to `LocalRunner` like `managed_runner`,
/// so the former `CloudAgent` variant is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveExecutorError {
    /// `ValueError("unknown agent executor")` (`policy.py:60`); a bare `ValueError`
    /// with no `code` attribute.
    UnknownExecutor,
}

impl std::fmt::Display for ResolveExecutorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown agent executor")
    }
}

impl std::error::Error for ResolveExecutorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

/// Routing matrix (`policy.py:57-72`, #2903).
///
/// `requested` is `None` when no override was given; `Some("")` falls through to
/// the default exactly like `None` (`requested or ...`, `policy.py:58`). A
/// legacy `managed_runner` value resolves to `local_runner` (the built-in
/// engine is a runner; new runs are pod-level) — the instance kill switch no
/// longer turns it away.
///
/// P1/P4 (agent naming convention): a stored `cloud_agent` value
/// resolves to `LocalRunner` too — the retired path is unreachable.
pub fn resolve_executor_kind(
    requested: Option<&str>,
    project_default: &str,
) -> Result<AgentExecutorKind, ResolveExecutorError> {
    let value = requested
        .filter(|r| !r.is_empty())
        .unwrap_or(project_default);
    let kind = AgentExecutorKind::from_value(value).ok_or(ResolveExecutorError::UnknownExecutor)?;
    if kind == AgentExecutorKind::PidashAgent || kind == AgentExecutorKind::RetiredCloudRun {
        return Ok(AgentExecutorKind::LocalRunner);
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

/// `agent_execution.py:118` (`09738d222`): online non-bundled runners
/// exist, but every one is operator-paused — the fix is to resume, not
/// to add one.
pub const LOCAL_RUNNER_PAUSED_REASON: &str = "local_runner_paused";

/// One `agent_executor_options` row (`agent_execution.py:112-128`), fields in dict
/// order (`kind`, `available`, `reason_code`, `deprecated`). `deprecated` is
/// true for the two legacy values older clients still write (`#2903`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutorOption {
    pub kind: AgentExecutorKind,
    pub available: bool,
    pub reason_code: String,
    pub deprecated: bool,
}

/// The `(available, reason_code)` verdict of L4's `pidash_agent_availability`
/// (`managed_runner/policy.py:76-96`), passed through verbatim
/// (`agent_execution.py:111`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PidashAgentAvailability {
    pub available: bool,
    pub reason_code: String,
}

/// Availability as explicit API data (`agent_execution.py:83-134`).
///
/// Rows are always `[cloud_agent, local_runner, managed_runner]`. The cloud row is
/// static (decision 4): always unavailable with `cloud_agent_unavailable`,
/// so the legacy frontend and the Django-parity tests keep passing with no
/// settings behind it. `local_runner_available` is the
/// [`LOCAL_RUNNER_AVAILABLE_EXISTS_SQL`] verdict (online pod member,
/// unpaused, engine unpaused); `local_runner_exists` is the
/// [`LOCAL_RUNNER_EXISTS_SQL`] verdict (online pod member, paused or
/// not) — consulted only when the runner is unavailable, exactly as
/// Python's `elif` runs the second `exists()` only then. `managed` is L4's
/// `pidash_agent_availability` verdict, passed through verbatim.
/// `deprecated` marks the two legacy executors: only `local_runner` is
/// selectable (design §6); availability is still computed for older
/// clients (NEWPIDASH-58).
pub fn agent_executor_options<F>(
    user: Option<UserFlags>,
    has_usable_llm_config: F,
    local_runner_available: bool,
    local_runner_exists: bool,
    managed: PidashAgentAvailability,
) -> [ExecutorOption; 3]
where
    F: FnOnce() -> bool,
{
    let _ = (user, has_usable_llm_config);
    let cloud_available = false;
    let cloud_reason = CloudAgentUnavailable::CODE.to_string();
    let local_reason = if local_runner_available {
        String::new()
    } else if local_runner_exists {
        LOCAL_RUNNER_PAUSED_REASON.to_string()
    } else {
        NO_LOCAL_RUNNER_REASON.to_string()
    };
    [
        ExecutorOption {
            kind: AgentExecutorKind::RetiredCloudRun,
            available: cloud_available,
            reason_code: cloud_reason,
            deprecated: true,
        },
        ExecutorOption {
            kind: AgentExecutorKind::LocalRunner,
            available: local_runner_available,
            reason_code: local_reason,
            deprecated: false,
        },
        ExecutorOption {
            kind: AgentExecutorKind::PidashAgent,
            available: managed.available,
            reason_code: managed.reason_code,
            deprecated: true,
        },
    ]
}

/// The local-runner legs of `agent_executor_options`
/// (`agent_execution.py:102-110`, #2903): runners that take pod work
/// only — ONLINE, in the project's pods, `pod_dispatch <> 'pinned_only'`
/// (pinned-only runners take no new issue work) and `revoked_at IS NULL`.
/// The *available* verdict adds unpaused (`paused_at IS NULL`) and not
/// engine-paused (a paused engine takes nothing); the *exists* verdict is
/// the online set alone. SeaORM (no raw SQL): the runner/pod reads are
/// small by construction (one project's pods).
async fn online_pod_work_runners(
    db: &impl sea_orm::ConnectionTrait,
    project_id: Uuid,
    workspace_id: Uuid,
) -> Result<Vec<runner::Model>, sea_orm::DbErr> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let pods = pod::Entity::find()
        .filter(pod::Column::ProjectId.eq(project_id))
        .all(db)
        .await?;
    let pod_ids: Vec<Uuid> = pods.into_iter().map(|row| row.id).collect();
    if pod_ids.is_empty() {
        return Ok(Vec::new());
    }
    runner::Entity::find()
        .filter(runner::Column::PodId.is_in(pod_ids))
        .filter(runner::Column::WorkspaceId.eq(workspace_id))
        .filter(runner::Column::Status.eq("online"))
        .filter(runner::Column::PodDispatch.ne("pinned_only"))
        .filter(runner::Column::RevokedAt.is_null())
        .all(db)
        .await
}

/// The *exists* verdict (`agent_execution.py:102-110`): any ONLINE
/// unterminated pod-member runner on the project's pods.
pub async fn local_runner_exists(
    db: &impl sea_orm::ConnectionTrait,
    project_id: Uuid,
    workspace_id: Uuid,
) -> Result<bool, sea_orm::DbErr> {
    Ok(!online_pod_work_runners(db, project_id, workspace_id)
        .await?
        .is_empty())
}

/// The *available* verdict (`agent_execution.py:112`, `09738d222` +
/// #2903): a pod-work runner that is unpaused (`paused_at IS NULL`)
/// and not engine-paused. An engine's machine pause needs the
/// `dev_machine` rows for the candidate engines only.
pub async fn local_runner_available_exists(
    db: &impl sea_orm::ConnectionTrait,
    project_id: Uuid,
    workspace_id: Uuid,
) -> Result<bool, sea_orm::DbErr> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let candidates = online_pod_work_runners(db, project_id, workspace_id).await?;
    let machine_ids: Vec<Uuid> = candidates
        .iter()
        .filter(|row| row.provisioning == "desktop_bundled")
        .filter_map(|row| row.dev_machine_id)
        .collect();
    let paused_machines: HashSet<Uuid> = if machine_ids.is_empty() {
        HashSet::new()
    } else {
        dev_machine::Entity::find()
            .filter(dev_machine::Column::Id.is_in(machine_ids))
            .all(db)
            .await?
            .into_iter()
            .filter(|machine| {
                machine.engine_paused_by_owner || !machine.engine_auto_pause_reason.is_empty()
            })
            .map(|machine| machine.id)
            .collect()
    };
    Ok(candidates.iter().any(|row| {
        row.paused_at.is_none()
            && !(row.provisioning == "desktop_bundled"
                && row
                    .dev_machine_id
                    .is_some_and(|id| paused_machines.contains(&id)))
    }))
}

// build_tool_plan / resolve_current_tool_names / ToolPlan /
// resolve_run_project / LinkedPrCtx / GitHub-binding SQL deleted
// in P3 step B.

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

    fn django_managed_settings() -> ManagedRunnerSettings {
        ManagedRunnerSettings {
            enabled: false,
            queued_max_age_secs: 43200,
            graceful_stop_secs: 30,
            sweep_interval_secs: 300,
            desktop_min_version: String::new(),
        }
    }

    fn managed(available: bool, reason: &str) -> PidashAgentAvailability {
        PidashAgentAvailability {
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

    // -- kill switches + effective executor + llm gate -----------------------

    #[test]
    fn kill_switches_read_the_settings() {
        // `cloud_agent_is_configured` died in P4: the retired row is
        // static (decision 4); only the managed switch reads settings.
        let managed = django_managed_settings();
        assert!(!managed_runner_is_enabled(&managed));
        let managed = ManagedRunnerSettings {
            enabled: true,
            ..django_managed_settings()
        };
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
        let matrix = fixture()["resolve_executor_kind_matrix"]
            .as_array()
            .expect("matrix")
            .clone();
        // requested=null inherits the default.
        assert_eq!(
            resolve_executor_kind(None, "local_runner"),
            Ok(AgentExecutorKind::LocalRunner)
        );
        // P1: an explicit cloud request resolves to local even when
        // configured — the retired path is unreachable.
        assert_eq!(
            resolve_executor_kind(Some("cloud_agent"), "local_runner"),
            Ok(AgentExecutorKind::LocalRunner)
        );
        // #2903: a legacy managed value resolves to local (the kill
        // switch no longer turns it away).
        assert_eq!(
            resolve_executor_kind(Some("managed_runner"), "local_runner"),
            Ok(AgentExecutorKind::LocalRunner)
        );
        assert_eq!(matrix[2]["result"], "local_runner");
        // Unknown values raise the bare ValueError message (no code).
        let err = resolve_executor_kind(Some("bogus"), "local_runner")
            .expect_err("unknown executor raises");
        assert_eq!(err, ResolveExecutorError::UnknownExecutor);
        assert_eq!(err.to_string(), "unknown agent executor");
        assert_eq!(matrix[3]["raises"], "ValueError");
        assert_eq!(matrix[3]["message"], "unknown agent executor");
        // P1: cloud on an unconfigured instance resolves to local instead
        // of raising. This intentionally diverges from the Python matrix
        // at matrix[4] (Django still gates on CLOUD_AGENT_ENABLED).
        assert_eq!(
            resolve_executor_kind(Some("cloud_agent"), "local_runner"),
            Ok(AgentExecutorKind::LocalRunner)
        );
        // A stored cloud default resolves to local, as does an explicit
        // local override over a cloud default.
        assert_eq!(
            resolve_executor_kind(None, "cloud_agent"),
            Ok(AgentExecutorKind::LocalRunner)
        );
        assert_eq!(
            resolve_executor_kind(Some("local_runner"), "cloud_agent"),
            Ok(AgentExecutorKind::LocalRunner)
        );
    }

    #[test]
    fn executor_resolution_edge_cases_match_python() {
        // `requested or default`: "" behaves as None (oracle-verified).
        assert_eq!(
            resolve_executor_kind(Some(""), "local_runner"),
            Ok(AgentExecutorKind::LocalRunner)
        );
        // An invalid project default fails the same membership check.
        assert_eq!(
            resolve_executor_kind(None, "bogus"),
            Err(ResolveExecutorError::UnknownExecutor)
        );
        // Error taxonomy (P3 step B: the cloud capability / required-tool
        // errors died with the tool plan): codes, messages,
        // std::error::Error impls.
        assert_eq!(CloudAgentUnavailable::CODE, "cloud_agent_unavailable");
        let cloud_err = CloudAgentUnavailable::new("m");
        assert_eq!(cloud_err.code(), CloudAgentUnavailable::CODE);
        assert_eq!(cloud_err.message(), "m");
        assert_eq!(cloud_err.to_string(), "m");
        let _: &dyn std::error::Error = &cloud_err;
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
            json!(["kind", "available", "reason_code", "deprecated"])
        );

        // The cloud row is static (decision 4): unavailable with the
        // retired code, whatever the user/LLM verdicts are.
        let rows = agent_executor_options(
            None,
            || panic!("no seam behind the static row"),
            true,
            true,
            managed(true, ""),
        );
        assert_eq!(
            rows.iter().map(|r| r.kind.value()).collect::<Vec<_>>(),
            ["cloud_agent", "local_runner", "managed_runner"]
        );
        // Row keys in order (`kind`, `available`, `reason_code`,
        // `deprecated`): struct serialization always follows field order.
        // `deprecated` is true for the two legacy values (#2903).
        assert!(rows[0].deprecated && rows[2].deprecated && !rows[1].deprecated);
        let text = serde_json::to_string(&rows[0]).expect("serializes");
        assert_eq!(
            text,
            r#"{"kind":"cloud_agent","available":false,"reason_code":"cloud_agent_unavailable","deprecated":true}"#
        );
        assert!(!rows[1].deprecated);
        assert!(rows[2].deprecated);
    }

    #[test]
    fn executor_options_cloud_and_local_legs() {
        let shape = &fixture()["agent_executor_options_shape"];
        // The cloud leg is static (decision 4): unavailable with the
        // retired code regardless of user or LLM verdict.
        let rows = agent_executor_options(
            Some(active_user()),
            || panic!("no seam behind the static row"),
            false,
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

        // Local leg: reason iff unavailable (the query itself is SQL, below).
        let rows = agent_executor_options(
            None,
            || panic!("no user"),
            false,
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
            None,
            || panic!("no user"),
            true,
            true,
            managed(true, ""),
        );
        assert!(rows[1].available);
        assert_eq!(rows[1].reason_code, "");

        // Local leg, paused: online runners exist but all are paused —
        // unavailable with the resume-the-runner reason (`09738d222`).
        let rows = agent_executor_options(
            None,
            || panic!("no user"),
            false,
            true,
            managed(true, ""),
        );
        assert!(!rows[1].available);
        assert_eq!(rows[1].reason_code, LOCAL_RUNNER_PAUSED_REASON);

        // Managed leg passes L4's verdict through verbatim.
        let rows = agent_executor_options(
            None,
            || panic!("no user"),
            true,
            true,
            managed(false, "desktop_not_connected"),
        );
        assert!(!rows[2].available);
        assert_eq!(rows[2].reason_code, "desktop_not_connected");
        assert_eq!(
            shape["managed_reason_passthrough"],
            "pidash_agent_availability(project, user) reason verbatim"
        );
    }

    #[test]
    fn local_runner_legs_count_pod_work() {
        // #2903: the local legs count ONLINE pod-member runners with
        // revoked_at NULL; the available leg adds paused_at NULL +
        // not engine-paused. Both are SeaORM now (no raw SQL consts).
        let shape = &fixture()["agent_executor_options_shape"];
        assert!(shape["local_excludes_pinned_only_and_revoked"]
            .as_str()
            .expect("golden")
            .contains("PINNED_ONLY"));
    }

}
