//! Dispatch service layer: cloud_agent + managed_runner policy (D-11, stage 5).
//!
//! * [`policy`] — executor policy (`core/agent_execution.py:34-128`,
//!   `cloud_agent/policy.py:11-196`).
//! * [`admission`] — admission + desktop permission + configuration checks
//!   (`cloud_agent/admission.py`, `cloud_agent/api.py`,
//!   `cloud_agent/checks.py`, `managed_runner/policy.py`,
//!   `managed_runner/checks.py`, `managed_runner/permissions.py`).
//!
//! Sibling L5+ issues add their own files under this module; L1 types live in
//! `pidash_types::dispatch` and L2 read shapes in `pidash_db::dispatch`.
//!
//! Fixture ids replayed by the unit tests alongside each module:
//! `rust-api/fixtures/dispatch/fx-disp-03-policy.golden.json` (FX-DISP-03) and
//! `rust-api/fixtures/dispatch/fx-disp-04-admission-checks.golden.json`
//! (FX-DISP-04).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod admission;
pub mod policy;

pub use admission::admission_bucket;
pub use admission::admission_retry_after;
pub use admission::cloud_agent_configuration_check;
pub use admission::consume_admission_token;
pub use admission::enforce_creation_rate;
pub use admission::is_desktop_session;
pub use admission::managed_runner_availability;
pub use admission::managed_runner_configuration_check;
pub use admission::user_admission_key;
pub use admission::workspace_admission_key;
pub use admission::AdmissionCache;
pub use admission::CloudAgentAdmissionError;
pub use admission::CloudAgentUnavailableBody;
pub use admission::ConfigurationError;
pub use admission::DeferredConsume;
pub use admission::DesktopSessionRequiredBody;
pub use admission::LlmProfile;
pub use admission::PhaseTemplate;
pub use admission::CLOUD_AGENT_UNAVAILABLE_HTTP_STATUS;
pub use admission::CONSUME_TIMEOUT_OVERHANG_SECS;
pub use admission::ENROLLED_MANAGED_RUNNERS_EXISTS_SQL;
pub use admission::HEARTBEAT_GRACE_SECS;
pub use admission::ONLINE_MANAGED_RUNNER_SQL;
pub use policy::agent_executor_options;
pub use policy::build_tool_plan;
pub use policy::cloud_agent_is_configured;
pub use policy::effective_executor_for_issue;
pub use policy::github_available_for_project;
pub use policy::managed_runner_is_enabled;
pub use policy::resolve_current_tool_names;
pub use policy::resolve_executor_kind;
pub use policy::resolve_run_project;
pub use policy::user_has_llm_config;
pub use policy::CloudAgentUnavailable;
pub use policy::CloudCapabilityUnavailable;
pub use policy::ExecutorOption;
pub use policy::LinkedPrCtx;
pub use policy::ManagedAvailability;
pub use policy::ProjectRef;
pub use policy::RequiredToolUnavailable;
pub use policy::ResolveExecutorError;
pub use policy::ToolPlan;
pub use policy::ToolPlanLimits;
pub use policy::UserFlags;
pub use policy::BINDING_REPO_SQL;
pub use policy::CODE_REVIEW_LINK_EXISTS_SQL;
pub use policy::CURRENT_ISSUE_WRITE_TOOLS;
pub use policy::GITHUB_BINDING_EXISTS_SQL;
pub use policy::GITHUB_TOOLS;
pub use policy::LLM_CONFIG_MISSING_REASON;
pub use policy::LOCAL_RUNNER_EXISTS_SQL;
pub use policy::NO_LOCAL_RUNNER_REASON;
pub use policy::READ_TOOLS;
pub use policy::REPEATABLE_WRITE_TOOLS;
pub use policy::TOOL_CATALOG_VERSION;
pub use policy::TOOL_PLAN_VERSION;
pub use policy::UNAVAILABLE_CAPABILITIES;
pub use policy::WRITE_TOOLS;
