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
//!   → [`managed_runner_configuration_check`] (E001/E004 + the data-driven
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

<<<<<<<< HEAD:rust-api/crates/services/src/dispatch/admission.rs
use super::policy::{managed_runner_is_enabled, ManagedAvailability, UserFlags};
use pidash_db::config::ManagedRunnerSettings;
use pidash_types::dispatch::{AgentExecutorKind, ManagedRunnerReason};
========
use super::executor_policy::{managed_runner_is_enabled, ManagedAvailability, UserFlags};
use pidash_db::config::ManagedRunnerSettings;
use pidash_types::agent_runs::{AgentExecutorKind, ManagedRunnerReason};
>>>>>>>> eb1cb02bc (refactor(p5): redo module moves with the spec names):rust-api/crates/services/src/run_creation/pidash_agent_availability.rs
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Admission error (`admission.py:15-19`)
// ---------------------------------------------------------------------------

// CloudAgentAdmissionError deleted in P3 step B (admission removal).

// Bucket math, key helpers, the admission cache seam, and the
// Redis implementation died in P4 (admission cascade).

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
/// / [`ONLINE_MANAGED_RUNNER_SQL`] verdicts. `paused_exists` is the
/// [`ENROLLED_PAUSED_MANAGED_RUNNERS_EXISTS_SQL`] verdict — evaluated only
/// when a runner is enrolled but none is online, exactly as Python's
/// `if` runs the paused probe last (`09738d222`).
pub fn managed_runner_availability<F>(
    managed: &ManagedRunnerSettings,
    user: Option<&UserFlags>,
    llm_profile: F,
    enrolled_exists: bool,
    online_exists: bool,
    paused_exists: bool,
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
        // `09738d222`: enrolled-but-unreachable splits into "paused"
        // (resume it) and "not connected" (open the desktop app).
        if paused_exists {
            return unavailable(ManagedRunnerReason::RUNNER_PAUSED);
        }
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
/// `pod` join + `pod.project_id = $2` (same join shape as the L3 local
/// EXISTS legs). Table `runner` is `db_table = "runner"`
/// (`runner/models.py:479`); `pod` is `db_table = "pod"`. Params: `$1` owner
/// id (uuid), `$2` project id (uuid), `$3` workspace id (uuid).
pub const ENROLLED_MANAGED_RUNNERS_EXISTS_SQL: &str = "SELECT 1 FROM runner INNER JOIN pod ON pod.id = runner.pod_id WHERE runner.owner_id = $1 AND runner.provisioning = 'desktop_bundled' AND pod.project_id = $2 AND runner.workspace_id = $3 AND runner.revoked_at IS NULL LIMIT 1";

/// `HEARTBEAT_GRACE` (`runner/services/matcher.py:44`): a runner counts as
/// online while its last heartbeat is within 90 seconds.
pub const HEARTBEAT_GRACE_SECS: i64 = 90;

/// The `online_managed_runner(...) is None` leg
/// (`managed_runner/policy.py:58-73,94-95`): the viewer's bundled runner that
/// can take work now — enrolled, `ONLINE`, heartbeat within
/// [`HEARTBEAT_GRACE_SECS`], unpaused (`09738d222` runs the lookup
/// through `assignable()`), newest heartbeat wins
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
pub const ONLINE_MANAGED_RUNNER_SQL: &str = "SELECT runner.id FROM runner INNER JOIN pod ON pod.id = runner.pod_id WHERE runner.owner_id = $1 AND runner.provisioning = 'desktop_bundled' AND pod.project_id = $2 AND runner.workspace_id = $3 AND runner.revoked_at IS NULL AND runner.status = 'online' AND runner.last_heartbeat_at >= $4 AND runner.paused_at IS NULL ORDER BY runner.last_heartbeat_at DESC LIMIT 1";

/// The paused tiebreaker (`managed_runner/policy.py`, `09738d222`):
/// whether the viewer owns any paused desktop-bundled runner on the
/// project's pod. Same enrolled shape as
/// [`ENROLLED_MANAGED_RUNNERS_EXISTS_SQL`] plus the `IS NOT NULL`
/// arm — no heartbeat or status gate, matching Python's plain
/// `.filter(paused_at__isnull=False)`. Params: `$1` owner id (uuid),
/// `$2` project id (uuid), `$3` workspace id (uuid).
pub const ENROLLED_PAUSED_MANAGED_RUNNERS_EXISTS_SQL: &str = "SELECT 1 FROM runner INNER JOIN pod ON pod.id = runner.pod_id WHERE runner.owner_id = $1 AND runner.provisioning = 'desktop_bundled' AND pod.project_id = $2 AND runner.workspace_id = $3 AND runner.revoked_at IS NULL AND runner.paused_at IS NOT NULL LIMIT 1";

/// Which managed-runner probe [`managed_runner_probe`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedRunnerProbe {
    /// [`ENROLLED_MANAGED_RUNNERS_EXISTS_SQL`].
    Enrolled,
    /// [`ONLINE_MANAGED_RUNNER_SQL`], heartbeat at or after the bound.
    Online(chrono::DateTime<chrono::Utc>),
    /// [`ENROLLED_PAUSED_MANAGED_RUNNERS_EXISTS_SQL`].
    Paused,
}

/// The SeaORM form of the three managed-runner probes: the matching
/// runner's id (the newest heartbeat first for [`ManagedRunnerProbe::Online`]),
/// or `None`. Non-revoked desktop-bundled runners the owner holds on the
/// project's pod (`runner INNER JOIN pod`), in this workspace.
pub async fn managed_runner_probe(
    db: &impl sea_orm::ConnectionTrait,
    probe: ManagedRunnerProbe,
    owner_id: uuid::Uuid,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
) -> Result<Option<uuid::Uuid>, sea_orm::DbErr> {
    use pidash_db::entity::{pod, runner};
    use sea_orm::{ColumnTrait, EntityTrait, JoinType, QueryFilter, QueryOrder, QuerySelect};
    let to_pod: sea_orm::RelationDef = runner::Entity::belongs_to(pod::Entity)
        .from(runner::Column::PodId)
        .to(pod::Column::Id)
        .into();
    let mut query = runner::Entity::find()
        .select_only()
        .column(runner::Column::Id)
        .join(JoinType::InnerJoin, to_pod)
        .filter(runner::Column::OwnerId.eq(owner_id))
        .filter(runner::Column::Provisioning.eq("desktop_bundled"))
        .filter(pod::Column::ProjectId.eq(project_id))
        .filter(runner::Column::WorkspaceId.eq(workspace_id))
        .filter(runner::Column::RevokedAt.is_null());
    match probe {
        ManagedRunnerProbe::Enrolled => {}
        ManagedRunnerProbe::Online(bound) => {
            query = query
                .filter(runner::Column::Status.eq("online"))
                .filter(runner::Column::LastHeartbeatAt.gte(bound))
                .filter(runner::Column::PausedAt.is_null())
                .order_by_desc(runner::Column::LastHeartbeatAt);
        }
        ManagedRunnerProbe::Paused => {
            query = query.filter(runner::Column::PausedAt.is_not_null());
        }
    }
    query.into_tuple().one(db).await
}

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

/// One Django `Error`/`Warning` (`id` + message) from a configuration check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigurationError {
    /// The check id (`cloud_agent.E005`, `managed.W001`, …).
    pub id: &'static str,
    /// The message, verbatim.
    pub message: String,
}

// cloud_agent_configuration_check deleted in P3 step B.

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

/// `managed_runner_configuration_check` (`managed_runner/checks.py:20-50`, #2903).
///
/// W001 (any non-local default is ignored with a warning, never refused),
/// E003/E004, plus the orchestration-`PHASES` × `MANAGED_RECIPES`
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
    if default_agent_executor != AgentExecutorKind::LocalRunner.value() {
        errors.push(ConfigurationError {
            id: "managed.W001",
            message: format!(
                "DEFAULT_AGENT_EXECUTOR='{default_agent_executor}' is ignored: new projects always use local_runner"
            ),
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
    use std::cell::Cell;
    

    static FIXTURE: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-04-admission-checks.golden.json");
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

    fn active_user() -> UserFlags {
        UserFlags {
            is_active: true,
            is_bot: false,
        }
    }

    // -- error shape (`admission.py:15-19`) ---------------------------------

    // -- bucket math and keys (`admission.py:63-76`) --------------------------

    // -- enforce_creation_rate (`admission.py:61-76`) -------------------------

    // -- CloudAgentUnavailableAPI (`api.py:5-8`) -------------------------------

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
        // #2903: W001 — a non-local default warns (never refuses). The
        // Django repr of a string is single-quoted.
        let errors = managed_runner_configuration_check(
            &django_managed_settings(),
            "managed_runner",
            &phases,
            recipes,
        );
        assert_eq!(
            check_pairs(&errors),
            vec![(
                "managed.W001",
                "DEFAULT_AGENT_EXECUTOR='managed_runner' is ignored: new projects always use local_runner",
            )]
        );
        // A local default is silent.
        let errors = managed_runner_configuration_check(
            &django_managed_settings(),
            "local_runner",
            &phases,
            recipes,
        );
        assert!(errors.is_empty());
        // E004: the fixture entry glosses the trigger ("(<= 0)");
        // — the message below is verbatim from `checks.py:30-37`.
        // E003 is gone with the cap (#2910): no check references the removed
        // setting, so a default-constructed settings value passes clean.
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
        // Django's phases against the real ported MANAGED_RECIPES: no
        // E002; only the W001 default-executor warning remains (#2903).
        let mut managed = django_managed_settings();
        managed.enabled = true;
        let errors = managed_runner_configuration_check(
            &managed,
            "managed_runner",
            &django_phases(),
            crate::prompting::recipes::MANAGED_RECIPES,
        );
        assert_eq!(
            errors,
            [ConfigurationError {
                id: "managed.W001",
                message: "DEFAULT_AGENT_EXECUTOR='managed_runner' is ignored: new projects \
                          always use local_runner"
                    .to_owned(),
            }]
        );
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
            false,
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
            let verdict = managed_runner_availability(
                &managed,
                viewer,
                || profile(true, ""),
                true,
                true,
                false,
            );
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
            false,
        );
        assert!(!verdict.available);
        assert_eq!(verdict.reason_code, ManagedRunnerReason::BYOK_UNSUPPORTED);
        // Gate 3b: empty profile reason falls back to llm_config_missing.
        let verdict = managed_runner_availability(
            &managed,
            Some(&user),
            || profile(false, ""),
            true,
            true,
            false,
        );
        assert!(!verdict.available);
        assert_eq!(verdict.reason_code, ManagedRunnerReason::LLM_CONFIG_MISSING);
        // Gate 4: enrolled but offline.
        let verdict = managed_runner_availability(
            &managed,
            Some(&user),
            || profile(true, ""),
            false,
            false,
            false,
        );
        assert!(!verdict.available);
        assert_eq!(
            verdict.reason_code,
            ManagedRunnerReason::NO_RUNNER_FOR_PROJECT
        );
        // Gate 5: enrolled, none online.
        let verdict = managed_runner_availability(
            &managed,
            Some(&user),
            || profile(true, ""),
            true,
            false,
            false,
        );
        assert!(!verdict.available);
        assert_eq!(verdict.reason_code, ManagedRunnerReason::NOT_CONNECTED);
        // Gate 5b: enrolled, none online, but at least one paused —
        // resume it instead of opening the desktop app (`09738d222`).
        let verdict = managed_runner_availability(
            &managed,
            Some(&user),
            || profile(true, ""),
            true,
            false,
            true,
        );
        assert!(!verdict.available);
        assert_eq!(verdict.reason_code, ManagedRunnerReason::RUNNER_PAUSED);
        // Pass: (True, "").
        let verdict = managed_runner_availability(
            &managed,
            Some(&user),
            || profile(true, ""),
            true,
            true,
            false,
        );
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
            "SELECT runner.id FROM runner INNER JOIN pod ON pod.id = runner.pod_id WHERE runner.owner_id = $1 AND runner.provisioning = 'desktop_bundled' AND pod.project_id = $2 AND runner.workspace_id = $3 AND runner.revoked_at IS NULL AND runner.status = 'online' AND runner.last_heartbeat_at >= $4 AND runner.paused_at IS NULL ORDER BY runner.last_heartbeat_at DESC LIMIT 1"
        );
        // The paused tiebreaker is the enrolled shape plus `IS NOT NULL`
        // (`09738d222`); params keep their positions.
        assert_eq!(
            ENROLLED_PAUSED_MANAGED_RUNNERS_EXISTS_SQL,
            "SELECT 1 FROM runner INNER JOIN pod ON pod.id = runner.pod_id WHERE runner.owner_id = $1 AND runner.provisioning = 'desktop_bundled' AND pod.project_id = $2 AND runner.workspace_id = $3 AND runner.revoked_at IS NULL AND runner.paused_at IS NOT NULL LIMIT 1"
        );
    }
}
