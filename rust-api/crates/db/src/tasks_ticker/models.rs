//! Agent ticker + scheduler table models (D-10, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/issue_agent_ticker.py:1-246`
//! (`IssueAgentTicker`, `TickerDisarmReason`, `jitter_seconds`,
//! `DEFAULT_INTERVAL_SECONDS`, `DEFAULT_MAX_TICKS`, `INFINITE_MAX_TICKS`,
//! `JITTER_FRACTION`) and `apps/api/pi_dash/db/models/scheduler.py:1-251`
//! (`Scheduler`, `SchedulerBinding`, `SchedulerSource`, `OutcomeMode`,
//! `OUTCOME_MODE_DIRECTIVES`, `outcome_mode_directive`,
//! `LAST_ERROR_MAX_LEN`). Adopts the Django-owned schema column-for-column;
//! migrations are not ported — Django stays schema owner until switchover.
//!
//! Column order in each `*_COLUMNS` const follows the Django `_meta` field
//! order recorded in `rust-api/fixtures/tasks_ticker/models/*.columns.json`
//! (FK entries use the Django attnames: `issue_id`, `workspace_id`, …).
//! Every application-level default below is Django-side (the live tables
//! carry no `column_default` in `information_schema`, as established for
//! D-01); Rust inserts must supply these values explicitly.
//!
//! # Reads are soft-delete scoped
//!
//! All three tables inherit the soft-delete marker (`deleted_at`) and the
//! default manager filters `deleted_at IS NULL`. Every read built from
//! these tables must apply [`crate::soft_delete::active_condition`] (or go
//! through the per-table read view); the tests pin this by rendering a
//! scoped `SELECT` per table. Partial unique constraints stay as they are
//! (tombstones are excluded by the `deleted_at IS NULL` condition, so
//! uninstall/reinstall does not collide).
//!
//! # Method seams
//!
//! The Python methods that dereference related rows take the resolved value
//! as a parameter here:
//!
//! * `pool_size()` reads `issue.project.agent_default_max_ticks` with a
//!   `getattr` default of `DEFAULT_MAX_TICKS`. The models layer exposes
//!   [`pool_size_or_default`]; callers pass the project column (or `None`
//!   when the row lacks the field).
//! * `effective_interval_seconds()` resolves the issue's stage through
//!   `orchestration.agent_phases.cadence_fields_for` and reads the project
//!   interval column with default `fields.default_interval` (currently
//!   10800 for all three stages). The models layer exposes
//!   [`resolve_project_interval`]; the stage→column mapping belongs to the
//!   queries layer (PIDASHCONV-207), which owns the project-row join.
//! * `scan_due_tickers` reproduces `effective_max_ticks` in SQL; change one
//!   and the other must change (PIDASHCONV-207 owns the SQL mirror).
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * `TickerDisarmReason` carries the cap-hit-only auto-pause gate: only
//!   `cap_hit` auto-pauses an In Progress issue (`maybe_apply_deferred_pause`).
//! * `SchedulerBinding` carries no duplicated run-status enum — read status
//!   off `last_run.status`.
//! * `pod` is late-bound: `NULL` means "use the project's default pod",
//!   resolved at fire time; a hard pod delete degrades via `SET_NULL`.
//! * `outcome_mode_directive` falls back to the `CREATE_ISSUE` directive for
//!   unknown values so a stale row never dispatches without guidance.
//! * `jitter_seconds` draws from the same uniform
//!   `[0, interval × JITTER_FRACTION)` distribution via the caller's RNG;
//!   the exact Python Mersenne-Twister stream is not reproduced across RNG
//!   implementations (see tests).

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Interval fallback (`issue_agent_ticker.py:35`): 3 h. Also the current
/// per-stage `default_interval` in `CADENCE_FIELDS` (all three stages).
pub const DEFAULT_INTERVAL_SECONDS: i64 = 10800;

/// One pool per issue (`issue_agent_ticker.py:36`); the `getattr` default
/// when the project row lacks `agent_default_max_ticks`.
pub const DEFAULT_MAX_TICKS: i32 = 10;

/// Infinite-pool sentinel (`issue_agent_ticker.py:37`).
pub const INFINITE_MAX_TICKS: i32 = -1;

/// Jitter width as a fraction of the interval (`issue_agent_ticker.py:38`).
pub const JITTER_FRACTION: f64 = 0.1;

/// Cap on `SchedulerBinding.last_error` (`scheduler.py:22`).
pub const LAST_ERROR_MAX_LEN: usize = 1000;

/// Django-level FK delete behavior (ORM-emulated; same shape as
/// `license::models::OnDelete`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OnDelete {
    /// `models.SET_NULL` — FK column must be nullable.
    SetNull,
    /// `models.CASCADE` — dependent rows are deleted with the parent.
    Cascade,
}

/// Why the ticker is currently disarmed (`issue_agent_ticker.py:41-61`).
///
/// Only [`TickerDisarmReason::CapHit`] auto-pauses an In Progress issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TickerDisarmReason {
    /// Armed (stored as `""`).
    #[default]
    None,
    /// The issue left the ticking bucket.
    LeftTickingState,
    /// A tick consumed the last run in the pool. The only auto-pausing reason.
    CapHit,
    /// The pool was already spent when the issue moved; never auto-pauses.
    PoolSpent,
    /// A terminal signal (`done` / `blocked` / `waiting_on_human`) disarmed it.
    TerminalSignal,
    /// A human disabled the clock.
    UserDisabled,
}

impl TickerDisarmReason {
    /// The stored string (`TextChoices` value).
    pub fn as_str(self) -> &'static str {
        match self {
            TickerDisarmReason::None => "",
            TickerDisarmReason::LeftTickingState => "left_ticking_state",
            TickerDisarmReason::CapHit => "cap_hit",
            TickerDisarmReason::PoolSpent => "pool_spent",
            TickerDisarmReason::TerminalSignal => "terminal_signal",
            TickerDisarmReason::UserDisabled => "user_disabled",
        }
    }

    /// All six values in declaration order (matches `enums.json`).
    pub const ALL: &[TickerDisarmReason] = &[
        TickerDisarmReason::None,
        TickerDisarmReason::LeftTickingState,
        TickerDisarmReason::CapHit,
        TickerDisarmReason::PoolSpent,
        TickerDisarmReason::TerminalSignal,
        TickerDisarmReason::UserDisabled,
    ];
}

/// Error for unknown disarm-reason strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownDisarmReason(pub String);

impl std::fmt::Display for UnknownDisarmReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown ticker disarm reason: {}", self.0)
    }
}

impl std::error::Error for UnknownDisarmReason {}

impl FromStr for TickerDisarmReason {
    type Err = UnknownDisarmReason;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "" => Ok(TickerDisarmReason::None),
            "left_ticking_state" => Ok(TickerDisarmReason::LeftTickingState),
            "cap_hit" => Ok(TickerDisarmReason::CapHit),
            "pool_spent" => Ok(TickerDisarmReason::PoolSpent),
            "terminal_signal" => Ok(TickerDisarmReason::TerminalSignal),
            "user_disabled" => Ok(TickerDisarmReason::UserDisabled),
            other => Err(UnknownDisarmReason(other.to_string())),
        }
    }
}

/// Scheduler definition source (`scheduler.py:25-27`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SchedulerSource {
    /// Built-in scheduler (the Django field default).
    #[default]
    Builtin,
    /// Installed from a manifest.
    Manifest,
}

impl SchedulerSource {
    /// The stored string.
    pub fn as_str(self) -> &'static str {
        match self {
            SchedulerSource::Builtin => "builtin",
            SchedulerSource::Manifest => "manifest",
        }
    }
}

/// Error for unknown scheduler-source strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownSchedulerSource(pub String);

impl std::fmt::Display for UnknownSchedulerSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown scheduler source: {}", self.0)
    }
}

impl std::error::Error for UnknownSchedulerSource {}

impl FromStr for SchedulerSource {
    type Err = UnknownSchedulerSource;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "builtin" => Ok(SchedulerSource::Builtin),
            "manifest" => Ok(SchedulerSource::Manifest),
            other => Err(UnknownSchedulerSource(other.to_string())),
        }
    }
}

/// What a scheduler run does with its findings (`scheduler.py:30-45`).
/// Stored per-install on the binding, not on the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum OutcomeMode {
    /// File issues (the Django field default).
    #[default]
    CreateIssue,
    /// Implement fixes, open PRs for review, never merge.
    ApplyFix,
    /// File issues and delegate the fix to the issue agent.
    FixAndReview,
}

impl OutcomeMode {
    /// The stored string.
    pub fn as_str(self) -> &'static str {
        match self {
            OutcomeMode::CreateIssue => "create_issue",
            OutcomeMode::ApplyFix => "apply_fix",
            OutcomeMode::FixAndReview => "fix_and_review",
        }
    }
}

/// Error for unknown outcome-mode strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownOutcomeMode(pub String);

impl std::fmt::Display for UnknownOutcomeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown outcome mode: {}", self.0)
    }
}

impl std::error::Error for UnknownOutcomeMode {}

impl FromStr for OutcomeMode {
    type Err = UnknownOutcomeMode;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "create_issue" => Ok(OutcomeMode::CreateIssue),
            "apply_fix" => Ok(OutcomeMode::ApplyFix),
            "fix_and_review" => Ok(OutcomeMode::FixAndReview),
            other => Err(UnknownOutcomeMode(other.to_string())),
        }
    }
}

/// Work-mode directive appended to a scheduler run's prompt, keyed by
/// [`OutcomeMode`] (`scheduler.py:52-93`, verbatim).
pub const OUTCOME_MODE_DIRECTIVES: &[(&str, &str)] = &[
    (
        "create_issue",
        "## Work mode: create issues\n\
         \n\
         For each distinct finding, file a Pi Dash issue with the `pidash` CLI:\n\
         \x20   pidash issue create --project <PROJ> --title \"<short summary>\" \\\n\
         \x20       --description \"<file path, line range, evidence, severity, suggested fix>\"\n\
         Before creating an issue, list existing open issues and skip any finding that already has a corresponding open issue (de-dupe by file + root cause, not by exact title). Do NOT modify code.",
    ),
    (
        "apply_fix",
        "## Work mode: apply fix\n\
         \n\
         For each finding you are confident about, implement the fix and open a pull request for human review \u{2014} do NOT merge it. Keep one PR per logical fix where practical. If a fix is risky, ambiguous, or larger than a focused change, do NOT force it: create a Pi Dash issue describing the finding instead (same form as create-issue mode).",
    ),
    (
        "fix_and_review",
        "## Work mode: file issue and delegate fix\n\
         \n\
         Do NOT modify code or open a pull request in this run \u{2014} the fix is delegated to the issue agent. For each distinct finding, do ALL of the following:\n\
         1. File a Pi Dash issue with the `pidash` CLI (de-dupe against existing open issues by file + root cause, as in create-issue mode), and note the issue identifier it returns. Write the description so an AI agent can implement the fix without re-investigating: file path(s) and line range, the evidence you observed, root cause, severity, a concrete suggested fix, and how to validate it:\n\
         \x20   pidash issue create --project <PROJ> --title \"<short summary>\" \\\n\
         \x20       --description \"<agent-ready technical details>\"\n\
         2. Move the issue to In Progress \u{2014} this automatically delegates it to the coding agent, which implements the fix and opens a pull request for human review:\n\
         \x20   pidash issue patch <IDENT> --state \"In Progress\"\n\
         If a finding is risky, ambiguous, or larger than a focused change, still file the issue but leave it in its default state (do NOT move it to In Progress) and describe the open questions in the issue description instead.",
    ),
];

/// Return the prompt directive for `mode` (`scheduler.py:96-104`).
///
/// Falls back to the `CREATE_ISSUE` directive for an unknown value so a
/// stale row can never dispatch a run with no work-mode guidance at all.
pub fn outcome_mode_directive(mode: &str) -> &'static str {
    OUTCOME_MODE_DIRECTIVES
        .iter()
        .find(|(key, _)| *key == mode)
        .map(|(_, directive)| *directive)
        .unwrap_or(OUTCOME_MODE_DIRECTIVES[0].1)
}

/// The project's per-issue pool (`issue_agent_ticker.py:200-203`).
///
/// Mirrors `getattr(issue.project, "agent_default_max_ticks",
/// DEFAULT_MAX_TICKS)`: `None` when the project row lacks the field.
/// `-1` means infinite.
pub fn pool_size_or_default(project_value: Option<i32>) -> i32 {
    project_value.unwrap_or(DEFAULT_MAX_TICKS)
}

/// Resolve the tick interval (`issue_agent_ticker.py:185-198`).
///
/// Mirrors `getattr(issue.project, fields.project_interval,
/// fields.default_interval)` after the stage→column resolution: the caller
/// passes the project column the current stage resolves through (or `None`
/// when the project row lacks it); the fallback is the stage default
/// (currently 10800 for all three stages).
pub fn resolve_project_interval(project_value: Option<i64>) -> i64 {
    project_value.unwrap_or(DEFAULT_INTERVAL_SECONDS)
}

/// Uniform random offset in `[0, interval × JITTER_FRACTION)`
/// (`issue_agent_ticker.py:64-72`).
///
/// Spreads out tick fires so bulk transitions do not re-cluster every cycle.
/// Returns `0.0` on non-positive intervals.
pub fn jitter_seconds<R: rand::Rng>(interval_seconds: i64, rng: &mut R) -> f64 {
    if interval_seconds <= 0 {
        return 0.0;
    }
    rng.gen_range(0.0..interval_seconds as f64 * JITTER_FRACTION)
}

/// `issue_agent_ticker` table (`issue_agent_ticker.py:75-176`).
///
/// Exactly one row per issue (`issue` is one-to-one); the clock is never
/// torn down and rebuilt on stage moves. Only `fire_tick`'s claim writes
/// `used` (PIDASHCONV-207 owns that write path).
pub mod issue_agent_ticker {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "issue_agent_ticker";

    /// Default `ORDER BY` (`Meta` declares no ordering).
    pub const ORDERING: &[&str] = &[];

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/tasks_ticker/models/issue_agent_ticker.columns.json`).
    /// FK columns use the Django attnames (`issue_id`, …).
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "issue_id",
        "used",
        "granted",
        "waited",
        "user_disabled",
        "next_run_at",
        "last_tick_at",
        "enabled",
        "disarm_reason",
        "pending_entry",
        "pending_entry_free",
        "pending_entry_actor_id",
        "pending_entry_trigger",
        "resume_parent_run_id",
    ];

    /// Due-scan index (`Meta.indexes`, `:171-176`).
    pub const DUE_INDEX_NAME: &str = "iaticker_enabled_next_run_idx";
    /// Columns of [`DUE_INDEX_NAME`].
    pub const DUE_INDEX: &[&str] = &["enabled", "next_run_at"];

    /// Integer budget defaults (`:97-109`, all `default=0`).
    pub const DEFAULT_USED: i32 = 0;
    /// See [`DEFAULT_USED`].
    pub const DEFAULT_GRANTED: i32 = 0;
    /// See [`DEFAULT_USED`].
    pub const DEFAULT_WAITED: i32 = 0;

    /// `user_disabled` default (`:111`).
    pub const DEFAULT_USER_DISABLED: bool = false;

    /// Persisted "is the clock live" answer (`:122`, `default=True`).
    pub const DEFAULT_ENABLED: bool = true;

    /// `disarm_reason` default (`:126-131`, empty string when armed).
    pub const DEFAULT_DISARM_REASON: &str = "";
    /// `disarm_reason` length bound (`:127`, `max_length=32`).
    pub const DISARM_REASON_MAX_LENGTH: usize = 32;

    /// `pending_entry` / `pending_entry_free` defaults (`:136`, `:140`).
    pub const DEFAULT_PENDING_ENTRY: bool = false;
    /// See [`DEFAULT_PENDING_ENTRY`].
    pub const DEFAULT_PENDING_ENTRY_FREE: bool = false;

    /// `pending_entry_trigger` default and bound (`:154`,
    /// `max_length=24`, empty for an agent-queued entry).
    pub const DEFAULT_PENDING_ENTRY_TRIGGER: &str = "";
    /// See [`DEFAULT_PENDING_ENTRY_TRIGGER`].
    pub const PENDING_ENTRY_TRIGGER_MAX_LENGTH: usize = 24;

    /// `issue` one-to-one: `CASCADE`, non-nullable (`:84-88`).
    pub const ISSUE_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `pending_entry_actor` FK: `SET_NULL`, nullable (`:144-150`).
    pub const PENDING_ENTRY_ACTOR_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `resume_parent_run` FK: `SET_NULL`, nullable (`:159-165`).
    pub const RESUME_PARENT_RUN_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// One ticker row. `disarm_reason` / `pending_entry_trigger` store `""`,
    /// never `NULL` (`blank=True`, not null); `next_run_at` / `last_tick_at`
    /// / `pending_entry_actor_id` / `resume_parent_run_id` are nullable.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct IssueAgentTicker {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub issue_id: uuid::Uuid,
        pub used: i32,
        pub granted: i32,
        pub waited: i32,
        pub user_disabled: bool,
        pub next_run_at: Option<chrono::DateTime<chrono::Utc>>,
        pub last_tick_at: Option<chrono::DateTime<chrono::Utc>>,
        pub enabled: bool,
        pub disarm_reason: String,
        pub pending_entry: bool,
        pub pending_entry_free: bool,
        pub pending_entry_actor_id: Option<uuid::Uuid>,
        pub pending_entry_trigger: String,
        pub resume_parent_run_id: Option<uuid::Uuid>,
    }

    impl IssueAgentTicker {
        /// Cap = project pool + `granted` + `waited`; `-1` is infinite
        /// (`issue_agent_ticker.py:205-214`). `pool` is
        /// [`super::pool_size_or_default`] applied to the project row.
        pub fn effective_max_ticks(&self, pool: i32) -> i32 {
            if pool == super::INFINITE_MAX_TICKS {
                return super::INFINITE_MAX_TICKS;
            }
            pool + self.granted + self.waited
        }

        /// Runs left in the pool, or `None` when the cap is infinite
        /// (`issue_agent_ticker.py:229-234`).
        pub fn remaining(&self, pool: i32) -> Option<i32> {
            let cap = self.effective_max_ticks(pool);
            if cap == super::INFINITE_MAX_TICKS {
                return None;
            }
            Some((cap - self.used).max(0))
        }

        /// Has this issue exhausted its pool?
        /// (`issue_agent_ticker.py:236-241`).
        pub fn cap_reached(&self, pool: i32) -> bool {
            let cap = self.effective_max_ticks(pool);
            if cap == super::INFINITE_MAX_TICKS {
                return false;
            }
            self.used >= cap
        }

        /// How many more `pidash issue wait` calls this issue may make
        /// (`issue_agent_ticker.py:216-227`): one extra project pool, `0`
        /// once spent — and `0` on an infinite pool, where waiting is
        /// meaningless.
        pub fn wait_allowance(&self, pool: i32) -> i32 {
            if pool == super::INFINITE_MAX_TICKS {
                return 0;
            }
            (pool - self.waited).max(0)
        }

        /// Back-compat spelling for external readers; `used` is the field
        /// (`issue_agent_ticker.py:243-245`).
        pub fn tick_count(&self) -> i32 {
            self.used
        }
    }

    impl std::fmt::Display for IssueAgentTicker {
        /// `__str__` (`:178-179`).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "IssueAgentTicker(issue={}, enabled={})",
                self.issue_id, self.enabled
            )
        }
    }
}

/// `schedulers` table (`scheduler.py:107-151`).
///
/// Reusable scheduler definitions, workspace-scoped; projects install them
/// via [`scheduler_binding::SchedulerBinding`].
pub mod scheduler {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "schedulers";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/tasks_ticker/models/scheduler.columns.json`, `scheduler`).
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "workspace_id",
        "slug",
        "name",
        "description",
        "prompt",
        "source",
        "is_enabled",
        "color",
    ];

    /// Partial unique `(workspace, slug)` when active
    /// (`:142-148`, tombstones excluded so uninstall/reinstall works).
    pub const UNIQUE_WHEN_ACTIVE: &[&[&str]] = &[&["workspace_id", "slug"]];
    /// Name of the partial unique constraint.
    pub const UNIQUE_WHEN_ACTIVE_NAME: &str = "scheduler_unique_workspace_slug_when_active";
    /// The partial condition, as Django spells it (`deleted_at__isnull=True`).
    pub const UNIQUE_WHEN_ACTIVE_CONDITION: &str = "deleted_at IS NULL";

    /// `slug` bound (`:119`, `max_length=64`).
    pub const SLUG_MAX_LENGTH: usize = 64;
    /// `name` bound (`:120`, `max_length=255`).
    pub const NAME_MAX_LENGTH: usize = 255;

    /// `description` default (`:121`, `blank=True`, `default=""`).
    pub const DEFAULT_DESCRIPTION: &str = "";

    /// `source` bound and default (`:123-127`, `default=builtin`).
    pub const SOURCE_MAX_LENGTH: usize = 16;
    /// See [`SOURCE_MAX_LENGTH`].
    pub const DEFAULT_SOURCE: &str = "builtin";

    /// `is_enabled` default (`:128`, `default=True`).
    pub const DEFAULT_IS_ENABLED: bool = true;

    /// Display color (`:132`, 7-char hex, default `#3b82f6`).
    pub const COLOR_MAX_LENGTH: usize = 7;
    /// See [`COLOR_MAX_LENGTH`].
    pub const DEFAULT_COLOR: &str = "#3b82f6";

    /// `workspace` FK: `CASCADE`, non-nullable (`:114-118`).
    pub const WORKSPACE_ON_DELETE: OnDelete = OnDelete::Cascade;

    /// One scheduler definition. `description` / `source` / `color` store
    /// `""`-style values, never `NULL`; only `deleted_at` is nullable
    /// besides the audit FKs.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Scheduler {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub slug: String,
        pub name: String,
        pub description: String,
        pub prompt: String,
        pub source: String,
        pub is_enabled: bool,
        pub color: String,
    }

    impl std::fmt::Display for Scheduler {
        /// `__str__` (`:150-151`).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Scheduler({}/{})", self.workspace_id, self.slug)
        }
    }
}

/// `scheduler_bindings` table (`scheduler.py:154-248`).
///
/// One install of a [`scheduler::Scheduler`] onto one project: the
/// per-install cadence, prompt context, outcome mode, and the runtime state
/// the beat fire loop reads.
pub mod scheduler_binding {
    use super::OnDelete;
    use serde::{Deserialize, Serialize};

    /// Django table name (`Meta.db_table`).
    pub const TABLE: &str = "scheduler_bindings";

    /// Default `ORDER BY` (`Meta.ordering = ("-created_at",)`).
    pub const ORDERING: &str = "-created_at";

    /// Columns in Django `_meta` field order (matches
    /// `fixtures/tasks_ticker/models/scheduler.columns.json`,
    /// `scheduler_binding`). Includes the inherited `WorkspaceBaseModel`
    /// `workspace` (non-null) + `project` (nullable) FKs.
    pub const COLUMNS: &[&str] = &[
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
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
        "next_run_at",
        "last_run_id",
        "last_error",
        "actor_id",
        "pod_id",
    ];

    /// Partial unique `(scheduler, project)` when active (`:236-241`).
    pub const UNIQUE_WHEN_ACTIVE: &[&[&str]] = &[&["scheduler_id", "project_id"]];
    /// Name of the partial unique constraint.
    pub const UNIQUE_WHEN_ACTIVE_NAME: &str = "scheduler_binding_unique_per_project_when_active";
    /// The partial condition, as Django spells it (`deleted_at__isnull=True`).
    pub const UNIQUE_WHEN_ACTIVE_CONDITION: &str = "deleted_at IS NULL";

    /// Due-scan index (`:243-248`).
    pub const DUE_INDEX_NAME: &str = "sched_binding_due_idx";
    /// Columns of [`DUE_INDEX_NAME`].
    pub const DUE_INDEX: &[&str] = &["enabled", "next_run_at"];

    /// `tzid` default and bound (`:176`, informational today, UTC expansion).
    pub const DEFAULT_TZID: &str = "UTC";
    /// See [`DEFAULT_TZID`].
    pub const TZID_MAX_LENGTH: usize = 64;

    /// Empty `rrule` = single-shot at `dtstart` (`:177`).
    pub const DEFAULT_RRULE: &str = "";

    /// `extra_context` default (`:181`, appended at run time).
    pub const DEFAULT_EXTRA_CONTEXT: &str = "";

    /// `enabled` default (`:182`).
    pub const DEFAULT_ENABLED: bool = true;

    /// `outcome_mode` bound and default (`:189-193`, `default=create_issue`
    /// matching the pre-existing builtin behavior).
    pub const OUTCOME_MODE_MAX_LENGTH: usize = 16;
    /// See [`OUTCOME_MODE_MAX_LENGTH`].
    pub const DEFAULT_OUTCOME_MODE: &str = "create_issue";

    /// `last_error` default (`:207`, short-circuit errors only).
    pub const DEFAULT_LAST_ERROR: &str = "";

    /// `scheduler` FK: `CASCADE`, non-nullable (`:162-166`).
    pub const SCHEDULER_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `last_run` FK: `SET_NULL`, nullable (`:199-205`).
    pub const LAST_RUN_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `actor` FK: `SET_NULL`, nullable (`:209-214`).
    pub const ACTOR_ON_DELETE: OnDelete = OnDelete::SetNull;
    /// `pod` FK: `SET_NULL`, nullable, late-bound default (`:223-229`).
    pub const POD_ON_DELETE: OnDelete = OnDelete::SetNull;

    /// Truncate `last_error` to [`super::LAST_ERROR_MAX_LEN`]
    /// (`scheduler.py:19-22` — one place so scanner, dispatch, and the
    /// runner-side terminate hook stay in sync).
    pub fn truncate_last_error(message: &str) -> &str {
        let mut end = message.len().min(super::LAST_ERROR_MAX_LEN);
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        &message[..end]
    }

    /// One binding row. `rrule` / `extra_context` / `outcome_mode` /
    /// `last_error` store `""`, never `NULL`; `rdates` / `exdates` are JSON
    /// arrays (fresh `[]` per row in Django); `next_run_at`, `last_run_id`,
    /// `actor_id`, `pod_id`, and `project_id` are nullable.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct SchedulerBinding {
        pub id: uuid::Uuid,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub updated_at: chrono::DateTime<chrono::Utc>,
        pub created_by_id: Option<uuid::Uuid>,
        pub updated_by_id: Option<uuid::Uuid>,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
        pub workspace_id: uuid::Uuid,
        pub project_id: Option<uuid::Uuid>,
        pub scheduler_id: uuid::Uuid,
        pub dtstart: chrono::DateTime<chrono::Utc>,
        pub tzid: String,
        pub rrule: String,
        pub rdates: serde_json::Value,
        pub exdates: serde_json::Value,
        pub extra_context: String,
        pub enabled: bool,
        pub outcome_mode: String,
        pub next_run_at: Option<chrono::DateTime<chrono::Utc>>,
        pub last_run_id: Option<uuid::Uuid>,
        pub last_error: String,
        pub actor_id: Option<uuid::Uuid>,
        pub pod_id: Option<uuid::Uuid>,
    }

    impl std::fmt::Display for SchedulerBinding {
        /// `__str__` (`:250-251`; Django renders `None` for a null FK).
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let project = self
                .project_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "None".to_string());
            write!(
                f,
                "SchedulerBinding(scheduler={}, project={})",
                self.scheduler_id, project
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soft_delete::active_condition;
    use rand::SeedableRng as _;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/tasks_ticker/models")
    }

    fn fixture(name: &str) -> serde_json::Value {
        let path = fixtures_dir().join(name);
        let body =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {name}: {e}"));
        serde_json::from_str(&body).expect("fixture is valid JSON")
    }

    /// Copy a const into an owned vec so assertions compare two runtime
    /// values (clippy denies asserting on constants directly).
    fn owned(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }

    /// Physical column names from a Django `_meta`-style `fields` array.
    fn fixture_columns(value: &serde_json::Value) -> Vec<String> {
        value["fields"]
            .as_array()
            .expect("fixture has fields array")
            .iter()
            .map(|f| {
                f["column"]
                    .as_str()
                    .expect("field has a column")
                    .to_string()
            })
            .collect()
    }

    fn field<'a>(value: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        value["fields"]
            .as_array()
            .expect("fixture has fields array")
            .iter()
            .find(|f| f["name"] == name)
            .unwrap_or_else(|| panic!("fixture has field {name}"))
    }

    #[test]
    fn ticker_columns_match_fixture() {
        let v = fixture("issue_agent_ticker.columns.json");
        assert_eq!(owned(issue_agent_ticker::COLUMNS), fixture_columns(&v));
        let table: &str = issue_agent_ticker::TABLE;
        assert_eq!(table, v["db_table"].as_str().unwrap());
        assert_eq!(table, "issue_agent_ticker");
        let ordering: &[&str] = issue_agent_ticker::ORDERING;
        assert!(ordering.is_empty());
        assert!(v["ordering"].as_array().unwrap().is_empty());
        let index = &v["indexes"][0];
        let name: &str = issue_agent_ticker::DUE_INDEX_NAME;
        assert_eq!(name, index["name"].as_str().unwrap());
        let index_fields: Vec<String> = index["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f.as_str().unwrap().to_string())
            .collect();
        assert_eq!(owned(issue_agent_ticker::DUE_INDEX), index_fields);
    }

    #[test]
    fn ticker_defaults_match_fixture() {
        let v = fixture("issue_agent_ticker.columns.json");
        let default = |name: &str| field(&v, name)["default"].clone();
        assert_eq!(default("used"), 0);
        assert_eq!(default("granted"), 0);
        assert_eq!(default("waited"), 0);
        let (used, granted, waited): (i32, i32, i32) = (
            issue_agent_ticker::DEFAULT_USED,
            issue_agent_ticker::DEFAULT_GRANTED,
            issue_agent_ticker::DEFAULT_WAITED,
        );
        assert_eq!((used, granted, waited), (0, 0, 0));
        assert_eq!(default("user_disabled"), false);
        let user_disabled: bool = issue_agent_ticker::DEFAULT_USER_DISABLED;
        assert!(!user_disabled);
        assert_eq!(default("enabled"), true);
        let enabled: bool = issue_agent_ticker::DEFAULT_ENABLED;
        assert!(enabled);
        assert_eq!(default("disarm_reason"), "");
        assert_eq!(field(&v, "disarm_reason")["max_length"], 32);
        let reason: &str = issue_agent_ticker::DEFAULT_DISARM_REASON;
        assert_eq!(reason, "");
        let reason_len: usize = issue_agent_ticker::DISARM_REASON_MAX_LENGTH;
        assert_eq!(reason_len, 32);
        assert_eq!(default("pending_entry"), false);
        assert_eq!(default("pending_entry_free"), false);
        let (pending_entry, pending_entry_free): (bool, bool) = (
            issue_agent_ticker::DEFAULT_PENDING_ENTRY,
            issue_agent_ticker::DEFAULT_PENDING_ENTRY_FREE,
        );
        assert!(!pending_entry);
        assert!(!pending_entry_free);
        assert_eq!(default("pending_entry_trigger"), "");
        assert_eq!(field(&v, "pending_entry_trigger")["max_length"], 24);
        let trigger: &str = issue_agent_ticker::DEFAULT_PENDING_ENTRY_TRIGGER;
        assert_eq!(trigger, "");
        let trigger_len: usize = issue_agent_ticker::PENDING_ENTRY_TRIGGER_MAX_LENGTH;
        assert_eq!(trigger_len, 24);
        // Nullability: clock + FK seams nullable, budget/flag/text not.
        for name in [
            "next_run_at",
            "last_tick_at",
            "pending_entry_actor",
            "resume_parent_run",
        ] {
            assert!(
                field(&v, name)["null"].as_bool().unwrap(),
                "{name} nullable"
            );
        }
        for name in ["used", "enabled", "disarm_reason", "pending_entry_trigger"] {
            assert!(
                !field(&v, name)["null"].as_bool().unwrap(),
                "{name} not null"
            );
        }
        assert_eq!(field(&v, "issue")["type"], "OneToOneField");
        assert_eq!(field(&v, "issue")["on_delete"], "CASCADE");
        assert_eq!(field(&v, "pending_entry_actor")["on_delete"], "SET_NULL");
        assert_eq!(field(&v, "resume_parent_run")["on_delete"], "SET_NULL");
        assert_eq!(issue_agent_ticker::ISSUE_ON_DELETE, OnDelete::Cascade);
        assert_eq!(
            issue_agent_ticker::PENDING_ENTRY_ACTOR_ON_DELETE,
            OnDelete::SetNull
        );
        assert_eq!(
            issue_agent_ticker::RESUME_PARENT_RUN_ON_DELETE,
            OnDelete::SetNull
        );
    }

    #[test]
    fn scheduler_columns_match_fixture() {
        let v = fixture("scheduler.columns.json");
        let s = &v["scheduler"];
        assert_eq!(owned(scheduler::COLUMNS), fixture_columns(s));
        let table: &str = scheduler::TABLE;
        assert_eq!(table, s["db_table"].as_str().unwrap());
        let ordering: &str = scheduler::ORDERING;
        assert_eq!(ordering, s["ordering"][0].as_str().unwrap());
        let constraint = &s["constraints"][0];
        let unique: Vec<Vec<String>> = scheduler::UNIQUE_WHEN_ACTIVE
            .iter()
            .map(|cols| owned(cols))
            .collect();
        let fixture_unique: Vec<String> = constraint["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                // Fixture names the Django field; the constraint is on the
                // physical column (FK attnames end in `_id`).
                let name = f.as_str().unwrap();
                field(s, name)["column"].as_str().unwrap().to_string()
            })
            .collect();
        assert_eq!(unique, vec![fixture_unique]);
        let name: &str = scheduler::UNIQUE_WHEN_ACTIVE_NAME;
        assert_eq!(name, constraint["name"].as_str().unwrap());
        let condition: &str = scheduler::UNIQUE_WHEN_ACTIVE_CONDITION;
        assert_eq!(condition, "deleted_at IS NULL");
        assert!(constraint["condition"]
            .as_str()
            .unwrap()
            .contains("deleted_at__isnull"));
        assert!(s["indexes"].as_array().unwrap().is_empty());

        let default = |name: &str| field(s, name)["default"].clone();
        assert_eq!(default("source"), "builtin");
        assert_eq!(default("is_enabled"), true);
        assert_eq!(default("color"), "#3b82f6");
        assert_eq!(default("description"), "");
        assert_eq!(field(s, "slug")["max_length"], 64);
        assert_eq!(field(s, "name")["max_length"], 255);
        assert_eq!(field(s, "source")["max_length"], 16);
        assert_eq!(field(s, "color")["max_length"], 7);
        let (source, enabled, color, description): (&str, bool, &str, &str) = (
            scheduler::DEFAULT_SOURCE,
            scheduler::DEFAULT_IS_ENABLED,
            scheduler::DEFAULT_COLOR,
            scheduler::DEFAULT_DESCRIPTION,
        );
        assert_eq!(
            (source, enabled, color, description),
            ("builtin", true, "#3b82f6", "")
        );
        let (slug_len, name_len, source_len, color_len): (usize, usize, usize, usize) = (
            scheduler::SLUG_MAX_LENGTH,
            scheduler::NAME_MAX_LENGTH,
            scheduler::SOURCE_MAX_LENGTH,
            scheduler::COLOR_MAX_LENGTH,
        );
        assert_eq!(
            (slug_len, name_len, source_len, color_len),
            (64, 255, 16, 7)
        );
        assert_eq!(scheduler::WORKSPACE_ON_DELETE, OnDelete::Cascade);
    }

    #[test]
    fn scheduler_binding_columns_match_fixture() {
        let v = fixture("scheduler.columns.json");
        let b = &v["scheduler_binding"];
        assert_eq!(owned(scheduler_binding::COLUMNS), fixture_columns(b));
        let table: &str = scheduler_binding::TABLE;
        assert_eq!(table, b["db_table"].as_str().unwrap());
        let ordering: &str = scheduler_binding::ORDERING;
        assert_eq!(ordering, b["ordering"][0].as_str().unwrap());
        let constraint = &b["constraints"][0];
        let unique: Vec<Vec<String>> = scheduler_binding::UNIQUE_WHEN_ACTIVE
            .iter()
            .map(|cols| owned(cols))
            .collect();
        let fixture_unique: Vec<String> = constraint["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                let name = f.as_str().unwrap();
                field(b, name)["column"].as_str().unwrap().to_string()
            })
            .collect();
        assert_eq!(unique, vec![fixture_unique]);
        let name: &str = scheduler_binding::UNIQUE_WHEN_ACTIVE_NAME;
        assert_eq!(name, constraint["name"].as_str().unwrap());
        assert!(constraint["condition"]
            .as_str()
            .unwrap()
            .contains("deleted_at__isnull"));
        let index = &b["indexes"][0];
        let index_name: &str = scheduler_binding::DUE_INDEX_NAME;
        assert_eq!(index_name, index["name"].as_str().unwrap());
        let index_fields: Vec<String> = index["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f.as_str().unwrap().to_string())
            .collect();
        assert_eq!(owned(scheduler_binding::DUE_INDEX), index_fields);

        let default = |name: &str| field(b, name)["default"].clone();
        assert_eq!(default("tzid"), "UTC");
        assert_eq!(default("rrule"), "");
        assert_eq!(default("extra_context"), "");
        assert_eq!(default("enabled"), true);
        assert_eq!(default("outcome_mode"), "create_issue");
        assert_eq!(default("last_error"), "");
        let (tzid, rrule, context, enabled, mode, error): (&str, &str, &str, bool, &str, &str) = (
            scheduler_binding::DEFAULT_TZID,
            scheduler_binding::DEFAULT_RRULE,
            scheduler_binding::DEFAULT_EXTRA_CONTEXT,
            scheduler_binding::DEFAULT_ENABLED,
            scheduler_binding::DEFAULT_OUTCOME_MODE,
            scheduler_binding::DEFAULT_LAST_ERROR,
        );
        assert_eq!(
            (tzid, rrule, context, enabled, mode, error),
            ("UTC", "", "", true, "create_issue", "")
        );
        assert_eq!(field(b, "tzid")["max_length"], 64);
        assert_eq!(field(b, "outcome_mode")["max_length"], 16);
        let (tzid_len, mode_len): (usize, usize) = (
            scheduler_binding::TZID_MAX_LENGTH,
            scheduler_binding::OUTCOME_MODE_MAX_LENGTH,
        );
        assert_eq!((tzid_len, mode_len), (64, 16));
        assert_eq!(field(b, "scheduler")["on_delete"], "CASCADE");
        assert_eq!(field(b, "last_run")["on_delete"], "SET_NULL");
        assert_eq!(field(b, "actor")["on_delete"], "SET_NULL");
        assert_eq!(field(b, "pod")["on_delete"], "SET_NULL");
        assert_eq!(scheduler_binding::SCHEDULER_ON_DELETE, OnDelete::Cascade);
        assert_eq!(scheduler_binding::LAST_RUN_ON_DELETE, OnDelete::SetNull);
        assert_eq!(scheduler_binding::ACTOR_ON_DELETE, OnDelete::SetNull);
        assert_eq!(scheduler_binding::POD_ON_DELETE, OnDelete::SetNull);
    }

    #[test]
    fn enums_match_fixture() {
        let v = fixture("enums.json");
        let values = |key: &str| -> Vec<String> {
            v[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["value"].as_str().unwrap().to_string())
                .collect()
        };
        let disarm: Vec<&str> = TickerDisarmReason::ALL.iter().map(|r| r.as_str()).collect();
        let expected: Vec<String> = values("ticker_disarm_reason");
        assert_eq!(disarm, expected);
        assert_eq!(disarm.len(), 6);
        for reason in TickerDisarmReason::ALL {
            assert_eq!(TickerDisarmReason::from_str(reason.as_str()), Ok(*reason));
        }
        assert_eq!(TickerDisarmReason::default(), TickerDisarmReason::None);
        assert!(TickerDisarmReason::from_str("bogus").is_err());

        let sources: Vec<String> = [SchedulerSource::Builtin, SchedulerSource::Manifest]
            .iter()
            .map(|s| s.as_str().to_string())
            .collect();
        assert_eq!(sources, values("scheduler_source"));
        assert_eq!(SchedulerSource::default(), SchedulerSource::Builtin);

        let modes: Vec<String> = [
            OutcomeMode::CreateIssue,
            OutcomeMode::ApplyFix,
            OutcomeMode::FixAndReview,
        ]
        .iter()
        .map(|m| m.as_str().to_string())
        .collect();
        assert_eq!(modes, values("outcome_mode"));
        assert_eq!(OutcomeMode::default(), OutcomeMode::CreateIssue);

        // Directives verbatim: every Rust entry equals the fixture text.
        let fixture_directives = v["outcome_mode_directives"].as_object().unwrap();
        assert_eq!(OUTCOME_MODE_DIRECTIVES.len(), fixture_directives.len());
        for (key, directive) in OUTCOME_MODE_DIRECTIVES {
            assert_eq!(*directive, fixture_directives[*key].as_str().unwrap());
        }
        // Unknown mode falls back to CREATE_ISSUE (fixture probes the prefix).
        let fallback_prefix = v["outcome_mode_directive_unknown_fallback"]
            .as_str()
            .unwrap();
        assert!(v["outcome_mode_directive_fallback_is_create_issue"]
            .as_bool()
            .unwrap());
        assert_eq!(
            outcome_mode_directive("create_issue"),
            OUTCOME_MODE_DIRECTIVES[0].1
        );
        assert_eq!(
            outcome_mode_directive("stale-mode"),
            OUTCOME_MODE_DIRECTIVES[0].1
        );
        assert!(outcome_mode_directive("stale-mode").starts_with(fallback_prefix));
    }

    #[test]
    fn constants_match_fixture() {
        let v = fixture("constants.json");
        let (interval, max_ticks, infinite): (i64, i32, i32) = (
            DEFAULT_INTERVAL_SECONDS,
            DEFAULT_MAX_TICKS,
            INFINITE_MAX_TICKS,
        );
        assert_eq!(interval, v["default_interval_seconds"].as_i64().unwrap());
        assert_eq!(max_ticks, v["default_max_ticks"].as_i64().unwrap() as i32);
        assert_eq!(infinite, v["infinite_max_ticks"].as_i64().unwrap() as i32);
        let jitter: f64 = JITTER_FRACTION;
        assert_eq!(jitter, v["jitter_fraction"].as_f64().unwrap());
        let last_error_len: usize = LAST_ERROR_MAX_LEN;
        assert_eq!(
            last_error_len,
            v["last_error_max_len"].as_u64().unwrap() as usize
        );
    }

    fn golden_ticker(used: i32, granted: i32, waited: i32) -> issue_agent_ticker::IssueAgentTicker {
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap();
        issue_agent_ticker::IssueAgentTicker {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            issue_id: uuid::Uuid::nil(),
            used,
            granted,
            waited,
            user_disabled: false,
            next_run_at: None,
            last_tick_at: None,
            enabled: true,
            disarm_reason: String::new(),
            pending_entry: false,
            pending_entry_free: false,
            pending_entry_actor_id: None,
            pending_entry_trigger: String::new(),
            resume_parent_run_id: None,
        }
    }

    #[test]
    fn budget_vectors_match_golden() {
        let v = fixture("budget_logic.golden.json");
        assert!(v["tick_count_alias_is_used"].as_bool().unwrap());
        for vector in v["vectors"].as_array().unwrap() {
            let pool = vector["pool"].as_i64().unwrap() as i32;
            let ticker = golden_ticker(
                vector["used"].as_i64().unwrap() as i32,
                vector["granted"].as_i64().unwrap() as i32,
                vector["waited"].as_i64().unwrap() as i32,
            );
            assert_eq!(
                ticker.effective_max_ticks(pool),
                vector["effective_max_ticks"].as_i64().unwrap() as i32,
                "effective_max_ticks for {vector}"
            );
            assert_eq!(
                ticker.wait_allowance(pool),
                vector["wait_allowance"].as_i64().unwrap() as i32,
                "wait_allowance for {vector}"
            );
            assert_eq!(
                ticker.remaining(pool),
                vector["remaining"].as_i64().map(|r| r as i32),
                "remaining for {vector}"
            );
            assert_eq!(
                ticker.cap_reached(pool),
                vector["cap_reached"].as_bool().unwrap(),
                "cap_reached for {vector}"
            );
            assert_eq!(ticker.tick_count(), ticker.used);
        }
        // Infinite pool: no cap, no remaining, no wait allowance.
        let infinite = golden_ticker(500, 0, 0);
        assert_eq!(
            infinite.effective_max_ticks(INFINITE_MAX_TICKS),
            INFINITE_MAX_TICKS
        );
        assert_eq!(infinite.remaining(INFINITE_MAX_TICKS), None);
        assert!(!infinite.cap_reached(INFINITE_MAX_TICKS));
        assert_eq!(infinite.wait_allowance(INFINITE_MAX_TICKS), 0);
        // Fresh pool-10 allowance is one full pool.
        assert_eq!(v["wait_allowance_pool10_fresh"].as_u64().unwrap(), 10);
        assert_eq!(golden_ticker(0, 0, 0).wait_allowance(10), 10);
    }

    #[test]
    fn jitter_contract_matches_python() {
        let v = fixture("budget_logic.golden.json");
        // Exact zero cases are deterministic across implementations.
        assert_eq!(v["jitter_seconds_0"].as_f64().unwrap(), 0.0);
        assert_eq!(v["jitter_seconds_negative"].as_f64().unwrap(), 0.0);
        let mut noop = rand::rngs::StdRng::seed_from_u64(0);
        assert_eq!(jitter_seconds(0, &mut noop), 0.0);
        assert_eq!(jitter_seconds(-5, &mut noop), 0.0);
        // Positive intervals draw uniform [0, interval × JITTER_FRACTION).
        // The exact Python stream (Mersenne Twister, seed 0 →
        // 911.9755… at 10800) is not reproduced across RNG
        // implementations; the distribution contract is.
        let bound = 10800.0 * JITTER_FRACTION;
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        for _ in 0..1000 {
            let sample = jitter_seconds(10800, &mut rng);
            assert!((0.0..bound).contains(&sample), "sample {sample} in range");
        }
    }

    #[test]
    fn pool_and_interval_resolution_match_getattr_defaults() {
        assert_eq!(pool_size_or_default(None), DEFAULT_MAX_TICKS);
        assert_eq!(pool_size_or_default(Some(25)), 25);
        assert_eq!(
            pool_size_or_default(Some(INFINITE_MAX_TICKS)),
            INFINITE_MAX_TICKS
        );
        assert_eq!(resolve_project_interval(None), DEFAULT_INTERVAL_SECONDS);
        assert_eq!(resolve_project_interval(Some(3600)), 3600);
    }

    #[test]
    fn last_error_truncates_at_char_boundary() {
        assert_eq!(scheduler_binding::truncate_last_error("boom"), "boom");
        let long = "e".repeat(LAST_ERROR_MAX_LEN + 40);
        assert_eq!(
            scheduler_binding::truncate_last_error(&long).len(),
            LAST_ERROR_MAX_LEN
        );
        // Multibyte safety: never split a char (Python slicing would panic
        // on a UTF-8 boundary in Rust; truncate below it instead).
        let wide = "é".repeat(600);
        let truncated = scheduler_binding::truncate_last_error(&wide);
        assert!(truncated.len() <= LAST_ERROR_MAX_LEN);
        assert_eq!(truncated.chars().count(), 500);
    }

    #[test]
    fn reads_are_soft_delete_scoped() {
        for table in [
            issue_agent_ticker::TABLE,
            scheduler::TABLE,
            scheduler_binding::TABLE,
        ] {
            let mut select = Query::select();
            select
                .column(Alias::new("id"))
                .from(Alias::new(table))
                .cond_where(active_condition());
            let sql = select.to_string(PostgresQueryBuilder);
            assert_eq!(
                sql,
                format!(r#"SELECT "id" FROM "{table}" WHERE "deleted_at" IS NULL"#)
            );
        }
    }

    #[test]
    fn display_matches_python_str() {
        let ticker = golden_ticker(3, 0, 0);
        assert_eq!(
            ticker.to_string(),
            format!("IssueAgentTicker(issue={}, enabled=true)", ticker.issue_id)
        );
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap();
        let scheduler_row = scheduler::Scheduler {
            id: uuid::Uuid::nil(),
            created_at: epoch,
            updated_at: epoch,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_id: uuid::Uuid::nil(),
            slug: "nightly".to_string(),
            name: "Nightly".to_string(),
            description: String::new(),
            prompt: "p".to_string(),
            source: "builtin".to_string(),
            is_enabled: true,
            color: "#3b82f6".to_string(),
        };
        assert_eq!(
            scheduler_row.to_string(),
            format!("Scheduler({}/nightly)", scheduler_row.workspace_id)
        );
    }
}
