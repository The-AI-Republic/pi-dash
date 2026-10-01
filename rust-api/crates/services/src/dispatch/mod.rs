//! Dispatch service layer: cloud_agent + managed_runner policy (D-11, stage 5).
//!
//! * [`policy`] — executor policy (`core/agent_execution.py:34-128`,
//!   `cloud_agent/policy.py:11-196`).
//!
//! Sibling L4+ issues add their own files under this module; L1 types live in
//! `pidash_types::dispatch` and L2 read shapes in `pidash_db::dispatch`.
//!
//! Fixture id replayed by the unit tests alongside each module:
//! `rust-api/fixtures/dispatch/fx-disp-03-policy.golden.json` (FX-DISP-03).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod policy;

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
