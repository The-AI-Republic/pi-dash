//! Dispatch service layer: cloud_agent + managed_runner policy (D-11, stage 5).
//!
//! * [`policy`] — executor policy (`core/agent_execution.py:34-128`,
//!   `cloud_agent/policy.py:11-196`).
//! * [`admission`] — admission + desktop permission + configuration checks
//!   (`cloud_agent/admission.py`, `cloud_agent/api.py`,
//!   `cloud_agent/checks.py`, `managed_runner/policy.py`,
//!   `managed_runner/checks.py`, `managed_runner/permissions.py`).
//! * [`execution`] — executor resolve (`creation.py`, `policy.py`);
//!   relocated from `pidash_jobs::dispatch` so the SeaORM-backed
//!   creation seam can call it (PIDASHCONV-1521). Collapsed by P3
//!   step B: resolve plus an empty tool plan.
//!
//! P3 step B deleted the run toolset (`tools`), the tool-issue reads
//! (`tool_issues`), and the cloud admission/creation gates'
//! public surface. The live create helpers moved to
//! [`crate::domain::issue_create_sql`].
//!
//! Sibling L6+ issues add their own files under this module; L1 types live in
//! `pidash_types::agent_runs` and L2 read shapes in `pidash_db::agent_runs`.
//!
//! Fixture ids replayed by the unit tests alongside each module:
//! the producer half of `fx-disp-03-policy.golden.json` (FX-DISP-03)
//! and the surviving half of `fx-disp-04-admission-checks.golden.json`
//! (FX-DISP-04). FX-DISP-05 is deleted (P3 step B).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod execution;
pub mod executor_policy;
pub mod pidash_agent_availability;

pub use pidash_agent_availability::is_desktop_session;
pub use pidash_agent_availability::managed_runner_availability;
pub use pidash_agent_availability::managed_runner_configuration_check;
pub use pidash_agent_availability::ConfigurationError;
pub use pidash_agent_availability::DesktopSessionRequiredBody;
pub use pidash_agent_availability::LlmProfile;
pub use pidash_agent_availability::PhaseTemplate;
pub use pidash_agent_availability::ENROLLED_MANAGED_RUNNERS_EXISTS_SQL;
pub use pidash_agent_availability::ENROLLED_PAUSED_MANAGED_RUNNERS_EXISTS_SQL;
pub use pidash_agent_availability::HEARTBEAT_GRACE_SECS;
pub use pidash_agent_availability::ONLINE_MANAGED_RUNNER_SQL;
pub use execution::execution_fields;
pub use execution::lock_workspace;
pub use execution::ActorScope;
pub use execution::DeferredAdmissionError;
pub use execution::ExecutionFields;
pub use execution::ExecutionFieldsError;
pub use execution::ExecutionInputs;
pub use execution::ProjectScope;
pub use execution::WORKSPACE_LOCK_SQL;
pub use executor_policy::agent_executor_options;
pub use executor_policy::effective_executor_for_issue;
pub use executor_policy::local_runner_available_exists;
pub use executor_policy::local_runner_exists;
pub use executor_policy::managed_runner_is_enabled;
pub use executor_policy::resolve_executor_kind;
pub use executor_policy::user_has_llm_config;
pub use executor_policy::CloudAgentUnavailable;
pub use executor_policy::ExecutorOption;
pub use executor_policy::ManagedAvailability;
pub use executor_policy::ResolveExecutorError;
pub use executor_policy::UserFlags;
pub use executor_policy::LLM_CONFIG_MISSING_REASON;
pub use executor_policy::LOCAL_RUNNER_PAUSED_REASON;
pub use executor_policy::NO_LOCAL_RUNNER_REASON;
