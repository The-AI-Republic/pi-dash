#![forbid(unsafe_code)]

//! Run toolset: `build_tools` gate, tool closures, GitHub adapter, model routing (D-11 L5).
//!
//! Port of `apps/api/pi_dash/cloud_agent/tools.py:24-554`,
//! `apps/api/pi_dash/cloud_agent/github_mcp.py:10-77`,
//! `apps/api/pi_dash/cloud_agent/model.py:11-22`,
//! `apps/api/pi_dash/ee/cloud_agent/toolsets.py:23-50`, and
//! `apps/api/pi_dash/ee/cloud_agent/model_provider.py:1-7`:
//!
//! * `ToolDenied` + `current_tool_call_id` (`tools.py:24-28`) →
//!   [`ToolDenied`] / [`ToolCallFailure`] + [`resolve_tool_call_id`].
//! * `_canonical` / `_fingerprint` / `_bounded` (`tools.py:31-43`) →
//!   [`canonical`] / [`fingerprint`] / [`bounded`] + [`collapse_replay`] /
//!   [`idempotency_key_hash`].
//! * `_scope` (`tools.py:46-73`) → [`check_scope`],
//!   [`scope_project_source`], [`SCOPE_RUN_SQL`],
//!   [`SCOPE_WORKSPACE_MEMBER_SQL`], [`PROJECT_READ_GATE_SQL`] (the write
//!   gate reuses the assistant's `PROJECT_WRITE_GATE_SQL`, the identical
//!   `check_project_role` call).
//! * `_audit` (`tools.py:76-157`) → [`audit_guards`] + [`replay_decision`] +
//!   [`write_admission`] + [`failure_error_code`] + [`tool_completed_event`]
//!   + the `LEDGER_*` SQL consts.
//! * `_issue_data` (`tools.py:160-171`) → [`IssueBrief`] / [`IssueDetail`].
//! * `_project_id` (`tools.py:174-179`) → [`scope_project_source`]
//!   (same chain; the id is read off the resolved source).
//! * `build_tools` (`tools.py:182-513`) → [`build_tools`] (granted gating) +
//!   [`tool_risk`] + one arg-guard constructor per closure
//!   ([`validate_comment_body`], [`validate_workpad_body`],
//!   [`validate_create_issue`], [`clamp_search_limit`],
//!   [`relation_write_args`], [`relations_source`],
//!   [`validate_github_path`], [`validate_github_ref`],
//!   [`validate_pr_aspect`], [`transition_same_state`],
//!   [`resolve_file_ref`], [`linked_pr_number`]) + the op SQL consts.
//! * `_relation_scope` / `_resolve_relation_refs` / `_relation_write`
//!   (`tools.py:379-414`) → [`OWN_PROJECT_ISSUES_PREDICATE`] (the visible
//!   pool reuses the assistant's `SCOPED_ISSUES_SQL`, the identical
//!   `member_project_issues` call) + [`unresolved_refs_message`] +
//!   [`relation_write_args`] + [`with_grouped_relations`].
//! * `_github_context` (`tools.py:516-554`) → [`check_github_binding`] +
//!   [`GITHUB_BINDING_SQL`] / [`GITHUB_INSTALLATION_SQL`] +
//!   [`GithubClientSpec`].
//! * `GITHUB_TOOL_NAMES` / `build_github_mcp` / `build_github_toolset`
//!   (`github_mcp.py:10-77`) → [`GITHUB_TOOL_NAMES`] +
//!   [`github_mcp_spec`] / [`github_toolset_spec`].
//! * `resolve_model_for_creator` (`model.py:11-22`) →
//!   [`resolve_model_for_creator`], delegating to the assistant seam.
//! * `resolve_model_for_run` (`model_provider.py:6-7`) →
//!   [`resolve_model_for_run`] (CE passthrough).
//! * CE `extra_toolsets_*` (`toolsets.py:23-50`) →
//!   [`extra_toolsets_enabled_for`] / [`resolve_extra_toolsets_for_run`] /
//!   [`extra_toolsets_schema_tool`].
//!
//! Translation notes:
//!
//! * This crate holds no DB or network handle (services convention), so the
//!   port is the pure decision surface plus SQL text with Postgres `$n`
//!   placeholders, which the caller (L6/L7) splices into its sqlx
//!   statement. Fixed enum values are literals, caller-supplied ids are
//!   `$n` params, each documented on its const. Behaviour that needs a
//!   live row (the op bodies, `operation(run)`) executes in the jobs
//!   layer; the guards, shapes, and statements it must honour live here.
//! * `serde_json` runs with `preserve_order` workspace-wide, so struct
//!   field order survives serialization (Python dict order is kept with
//!   serde structs, never `json!` maps) — but [`canonical`] sorts keys
//!   explicitly instead of relying on it.
//! * `json.dumps(..., ensure_ascii=True)` escapes every non-ASCII char and
//!   DEL as lowercase `\uXXXX` (astral chars as surrogate pairs); serde
//!   emits them raw, so [`canonical`] re-escapes them in a post-pass,
//!   verified against CPython.
//! * `len()` / slicing count code points, and `str.strip()` also strips
//!   U+001C–U+001F (which Rust `trim` keeps): length guards use
//!   `chars().count()`, truncation uses [`truncate_chars`], blank checks
//!   use [`py_stripped`] (Porting guide semantic traps).
//! * EE-overlayable seams and sibling-domain verdicts arrive as inputs,
//!   never reimplemented: `is_workspace_member` / `check_project_role`
//!   (`core/permissions.py`, SQL owned here), `member_project_issues`
//!   (reused SQL), `validate_relation_type` / `resolve_refs` / `relate` /
//!   `unrelate` / `grouped_relations` (D-12 orchestration, `FnOnce`
//!   closures or precomputed values so the Python short-circuit structure
//!   is preserved), `to_safe_html` (assistant markdown),
//!   `strip_tags`-derived columns, the GitHub client calls, and
//!   `handle_issue_state_transition` (D-12).
//! * `granted()` (`tools.py:197-198`) is dead code in Python (the return
//!   at `tools.py:512-513` uses the catalog comprehension directly) and
//!   is not ported. The comprehension would also grant the private
//!   helper-locals (`_relation_scope`, …) if named in `allowed`, but
//!   `allowed` always derives from the tool plan (L3: `build_tool_plan` /
//!   `resolve_current_tool_names` emit catalog names only), so [`build_tools`]
//!   grants the 15 catalog tools and the closed catalog is not widened.
//! * The tool-plan snapshot discipline (`toolsets.py:23-31`) is honoured:
//!   [`extra_toolsets_enabled_for`] is read at creation (L3 takes it as a
//!   seam verdict) and never consulted here at execution time.
//!
//! Fixture: `rust-api/fixtures/dispatch/fx-disp-05-tools.golden.json`
//! (FX-DISP-05).
//!
//! Ported quirks (translate as-is):
//!
//! * The create tool's `MAX(sequence_id)+1` computation is discarded:
//!   `Issue.save()` (`db/models/issue.py:312-341`) overwrites `sequence_id`
//!   from the advisory-locked `IssueSequence` table before insert.
//!   [`CREATE_SEQ_MAX_SQL`] still runs (the query executes); the effective
//!   sequence comes from [`CREATE_SEQ_SCAN_SQL`].
//! * `Issue.save()` stamps `completed_at` only when the tool supplied a
//!   state; a save-resolved default state skips the stamping
//!   (`issue.py:289-310`).
//! * The write path has no failure marking: an op exception leaves the
//!   ledger row `SUBMITTED` (only the read path writes `FAILED` +
//!   `error_code`).
//! * `github_get_linked_pull_request` raises `RuntimeError`
//!   (`no_linked_pull_request`), not `ToolDenied`, when no link exists.
//! * The linked-review row key `"number"` holds the *string*
//!   `external_iid`, not an integer.

use pidash_db::config::CloudAgentSettings;
use pidash_db::dispatch::ToolCallStatus;
use pidash_types::assistant::errors::AssistantError;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

use super::policy::{READ_TOOLS, REPEATABLE_WRITE_TOOLS, WRITE_TOOLS};
use crate::assistant::llm::ModelRef;

// ---------------------------------------------------------------------------
// Denial taxonomy (`tools.py:24-28`)
// ---------------------------------------------------------------------------

/// `ToolDenied` (`tools.py:24-25`): a `RuntimeError` in Python whose message
/// is the denial code. Every code below is a verbatim message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct ToolDenied {
    code: &'static str,
}

impl ToolDenied {
    /// `run.cancel_requested_at` is set (`tools.py:55-56`).
    pub const RUN_CANCELLED: &'static str = "run_cancelled";
    /// Creator inactive/bot/non-member, or the project role check failed
    /// (`tools.py:57-72`).
    pub const ACTOR_NO_LONGER_AUTHORIZED: &'static str = "actor_no_longer_authorized";
    /// `CLOUD_AGENT_ENABLED` is off (`tools.py:81-82`).
    pub const CLOUD_AGENT_DISABLED: &'static str = "cloud_agent_disabled";
    /// The tool is in `CLOUD_AGENT_DISABLED_TOOLS` (`tools.py:83-84`).
    pub const TOOL_DISABLED: &'static str = "tool_disabled";
    /// A write while `CLOUD_AGENT_WRITES_ENABLED` is off (`tools.py:85-86`).
    pub const WRITES_DISABLED: &'static str = "writes_disabled";
    /// An MCP github-server call while `CLOUD_AGENT_GITHUB_TOOLS_ENABLED`
    /// is off (`tools.py:87-88`).
    pub const GITHUB_TOOLS_DISABLED: &'static str = "github_tools_disabled";
    /// The ledger row exists under another request fingerprint
    /// (`tools.py:91-92`).
    pub const TOOL_CALL_FINGERPRINT_MISMATCH: &'static str = "tool_call_fingerprint_mismatch";
    /// The ledger row exists but has no replayable success
    /// (`tools.py:95`).
    pub const TOOL_CALL_ALREADY_SUBMITTED: &'static str = "tool_call_already_submitted";
    /// The run exhausted `CLOUD_AGENT_WRITE_CALL_LIMIT` (`tools.py:99-103`).
    pub const WRITE_LIMIT: &'static str = "write_limit";
    /// A non-repeatable write tool was already used by this run
    /// (`tools.py:104-108`).
    pub const WRITE_TOOL_ALREADY_USED: &'static str = "write_tool_already_used";
    /// No verified github binding for the run's project
    /// (`tools.py:528-540`).
    pub const GITHUB_BINDING_UNAVAILABLE: &'static str = "github_binding_unavailable";
    /// No verified app installation behind the binding
    /// (`tools.py:543-549`).
    pub const GITHUB_INSTALLATION_UNAVAILABLE: &'static str = "github_installation_unavailable";

    /// `ToolDenied(code)`.
    pub fn new(code: &'static str) -> Self {
        ToolDenied { code }
    }

    /// The denial code (`str(exc)`).
    pub fn code(&self) -> &'static str {
        self.code
    }
}

/// What a tool call can fail with, by Python exception type: `ToolDenied`
/// for denials, `ValueError` for invalid arguments, `RuntimeError` for
/// operational failures (`tool_result_too_large`, `no_linked_pull_request`,
/// client errors). The read path records [`ToolCallFailure::python_type_name`]
/// as the ledger `error_code` (`tools.py:145-150`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCallFailure {
    /// `ToolDenied(code)`.
    Denied(ToolDenied),
    /// `ValueError(message)` (arg guards, unknown refs, bad external ids).
    InvalidArg(String),
    /// `RuntimeError(message)` (oversize results, missing PR link, …).
    Failed(String),
}

impl ToolCallFailure {
    /// `type(exc).__name__` for the ledger `error_code`.
    pub fn python_type_name(&self) -> &'static str {
        match self {
            ToolCallFailure::Denied(_) => "ToolDenied",
            ToolCallFailure::InvalidArg(_) => "ValueError",
            ToolCallFailure::Failed(_) => "RuntimeError",
        }
    }

    /// `str(exc)`.
    pub fn message(&self) -> &str {
        match self {
            ToolCallFailure::Denied(denied) => denied.code(),
            ToolCallFailure::InvalidArg(message) | ToolCallFailure::Failed(message) => message,
        }
    }
}

impl From<ToolDenied> for ToolCallFailure {
    fn from(denied: ToolDenied) -> Self {
        ToolCallFailure::Denied(denied)
    }
}

impl std::fmt::Display for ToolCallFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for ToolCallFailure {}

// ---------------------------------------------------------------------------
// Canonical bytes, fingerprints, bounds (`tools.py:31-43`)
// ---------------------------------------------------------------------------

/// Sort an owned JSON value's object keys recursively, so [`canonical`]
/// does not depend on the serializer's map ordering.
fn sorted_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<&str, Value> = map
                .iter()
                .map(|(key, val)| (key.as_str(), sorted_value(val)))
                .collect();
            Value::Object(
                sorted
                    .into_iter()
                    .map(|(key, val)| (key.to_owned(), val))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted_value).collect()),
        scalar => scalar.clone(),
    }
}

/// Re-escape what serde leaves raw but CPython `ensure_ascii=True` escapes:
/// DEL and every non-ASCII char become lowercase `\uXXXX` (astral chars as
/// surrogate pairs). Everything else in serde's output is already ASCII and
/// correctly escaped, including the backslash introducers themselves.
fn escape_ascii(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch < '\u{20}' || ch == '"' || ch == '\\' {
            // Unreachable from serde output (it escapes these itself), kept
            // literal so a future serializer change fails loudly in tests
            // instead of double-escaping.
            out.push(ch);
        } else if (ch as u32) < 0x7f {
            out.push(ch);
        } else if (ch as u32) == 0x7f {
            out.push_str("\\u007f");
        } else if (ch as u32) < 0x10000 {
            out.push_str(&format!("\\u{:04x}", ch as u32));
        } else {
            let v = ch as u32 - 0x10000;
            out.push_str(&format!(
                "\\u{:04x}\\u{:04x}",
                0xd800 + (v >> 10),
                0xdc00 + (v & 0x3ff)
            ));
        }
    }
    out
}

/// `_canonical` (`tools.py:31-32`):
/// `json.dumps(value, sort_keys=True, separators=(",", ":"),
/// default=str).encode()`.
///
/// Keys sort recursively; separators are compact; non-ASCII/DEL escape as
/// lowercase `\uXXXX`. `default=str` has no Rust-side trigger: inputs are
/// `serde_json::Value`, already JSON (ids stringified by the caller, as
/// `str(run_id)` is in Python). Values from parsed JSON are always finite,
/// the only inputs `serde_json::to_string` accepts. Floats render via
/// serde, which matches CPython except for exponent padding on extreme
/// magnitudes (`1e300` vs `1e+300`) — tool values are ints, strings,
/// bools and nulls in practice.
pub fn canonical(value: &Value) -> Vec<u8> {
    let raw = serde_json::to_string(&sorted_value(value)).expect("tool values are finite JSON");
    escape_ascii(&raw).into_bytes()
}

/// `_fingerprint` (`tools.py:35-36`): lowercase sha256 hex of [`canonical`].
pub fn fingerprint(value: &Value) -> String {
    Sha256::digest(canonical(value))
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `_bounded` overflow message (`tools.py:42`).
pub const MSG_TOOL_RESULT_TOO_LARGE: &str = "tool_result_too_large";

/// `_bounded` (`tools.py:39-43`): return the value unchanged when its
/// canonical bytes fit `CLOUD_AGENT_MAX_TOOL_RESULT_BYTES`, else
/// `RuntimeError("tool_result_too_large")`.
pub fn bounded(value: Value, max_bytes: i64) -> Result<Value, ToolCallFailure> {
    if canonical(&value).len() as i64 > max_bytes {
        return Err(ToolCallFailure::Failed(
            MSG_TOOL_RESULT_TOO_LARGE.to_owned(),
        ));
    }
    Ok(value)
}

/// Replay-collapse threshold (`tools.py:124,152`): canonical results over
/// 4096 bytes store `{"ok": true}` as the replay value.
pub const REPLAY_COLLAPSE_BYTES: usize = 4096;

/// Whether a result of `canonical_len` bytes collapses in the ledger.
pub fn collapse_replay(canonical_len: usize) -> bool {
    canonical_len > REPLAY_COLLAPSE_BYTES
}

/// The collapsed replay value (`tools.py:124,152`).
pub fn collapsed_replay() -> Value {
    Value::Object([("ok".to_owned(), Value::Bool(true))].into_iter().collect())
}

/// `idempotency_key_hash` (`tools.py:118`):
/// `_fingerprint({"run": str(run_id), "tool": tool_name})`.
pub fn idempotency_key_hash(run_id: &str, tool_name: &str) -> String {
    let mut args = serde_json::Map::new();
    args.insert("run".to_owned(), Value::String(run_id.to_owned()));
    args.insert("tool".to_owned(), Value::String(tool_name.to_owned()));
    fingerprint(&Value::Object(args))
}

// ---------------------------------------------------------------------------
// Tool catalog + granted gating (`tools.py:182-199,512-513`)
// ---------------------------------------------------------------------------

/// The closed tool catalog: the 15 closures `build_tools` defines, in
/// `READ_TOOLS` then `WRITE_TOOLS` order (L3, `policy.py:11-29`).
pub const TOOL_CATALOG: [&str; 15] = [
    "pidash_get_current_issue",
    "pidash_list_current_issue_comments",
    "pidash_list_project_states",
    "pidash_search_project_issues",
    "pidash_get_project_issue",
    "pidash_list_linked_code_reviews",
    "pidash_list_issue_relations",
    "github_get_file",
    "github_get_linked_pull_request",
    "pidash_add_current_issue_comment",
    "pidash_update_current_issue_workpad",
    "pidash_transition_current_issue",
    "pidash_create_project_issue",
    "pidash_relate_issues",
    "pidash_unrelate_issues",
];

/// `GITHUB_TOOL_NAMES` (`github_mcp.py:10`).
pub const GITHUB_TOOL_NAMES: [&str; 2] = ["github_get_file", "github_get_linked_pull_request"];

/// Tool risk (`tools.py:80` compares `risk == "write"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRisk {
    /// `"read"`.
    Read,
    /// `"write"`.
    Write,
}

impl ToolRisk {
    /// The `risk` column value.
    pub fn as_str(self) -> &'static str {
        match self {
            ToolRisk::Read => "read",
            ToolRisk::Write => "write",
        }
    }
}

/// The catalog risk of a tool name, or `None` outside the catalog.
pub fn tool_risk(name: &str) -> Option<ToolRisk> {
    if READ_TOOLS.contains(&name) {
        Some(ToolRisk::Read)
    } else if WRITE_TOOLS.contains(&name) {
        Some(ToolRisk::Write)
    } else {
        None
    }
}

/// `build_tools` granted gating (`tools.py:512-513`):
/// `[catalog[name] for name in sorted(allowed) if name in catalog]` —
/// granted names in byte order, ungranted names dropped. Duplicates in
/// `allowed` are preserved, as Python's comprehension preserves them.
/// The runtime maps each granted name to its closure; `source` /
/// `server_key` ride into [`audit_guards`] and the ledger builders.
pub fn build_tools(allowed: &[&str]) -> Vec<&'static str> {
    let mut granted: Vec<&'static str> = allowed
        .iter()
        .filter_map(|name| TOOL_CATALOG.iter().find(|tool| *tool == name).copied())
        .collect();
    granted.sort_unstable();
    granted
}

// ---------------------------------------------------------------------------
// Scope (`tools.py:46-73`)
// ---------------------------------------------------------------------------

/// Project role values (`core/permissions.py:23-25`).
pub const ROLE_ADMIN: i32 = 20;
/// Project role values (`core/permissions.py:23-25`).
pub const ROLE_MEMBER: i32 = 15;
/// Project role values (`core/permissions.py:23-25`).
pub const ROLE_GUEST: i32 = 5;

/// Roles `_scope` requires (`tools.py:70`): writes need admin/member.
pub const SCOPE_ROLES_WRITE: [i32; 2] = [ROLE_ADMIN, ROLE_MEMBER];
/// Roles `_scope` requires (`tools.py:70`): reads also allow guests.
pub const SCOPE_ROLES_READ: [i32; 3] = [ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST];

/// The roles `_scope` passes to `check_project_role` for the write bit
/// (`tools.py:70`).
pub fn scope_roles(write: bool) -> &'static [i32] {
    if write {
        &SCOPE_ROLES_WRITE
    } else {
        &SCOPE_ROLES_READ
    }
}

/// Which run back-pointer supplied the project (`tools.py:63-69`,
/// `_project_id` at `tools.py:174-179`): the bound issue's project, else
/// the scheduler binding's, else the pod's. The pod FK is non-nullable
/// (`runner/models.py:898-902`), so the chain is total.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSource {
    /// `run.work_item.project`.
    WorkItem,
    /// `run.scheduler_binding.project`.
    SchedulerBinding,
    /// `run.pod.project`.
    Pod,
}

/// `_project_id` resolution order (`tools.py:174-179`).
pub fn scope_project_source(
    work_item_id: Option<Uuid>,
    scheduler_binding_id: Option<Uuid>,
) -> ProjectSource {
    if work_item_id.is_some() {
        ProjectSource::WorkItem
    } else if scheduler_binding_id.is_some() {
        ProjectSource::SchedulerBinding
    } else {
        ProjectSource::Pod
    }
}

/// The actor verdicts `_scope` reads off the run row and the membership
/// tables (`tools.py:55-62`). `role_ok` arrives separately because it
/// needs the resolved project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeActorVerdicts {
    /// `run.cancel_requested_at` is set.
    pub cancel_requested: bool,
    /// `run.created_by.is_active`.
    pub creator_active: bool,
    /// `run.created_by.is_bot`.
    pub creator_is_bot: bool,
    /// `is_workspace_member(run.created_by, run.workspace_id)`
    /// ([`SCOPE_WORKSPACE_MEMBER_SQL`]).
    pub workspace_member: bool,
}

/// `_scope` (`tools.py:46-73`): cancel → actor → project → role, in order.
/// `role_ok` is the `check_project_role` verdict for
/// [`scope_roles`] (`write`) over the resolved project
/// ([`PROJECT_READ_GATE_SQL`] for reads, the assistant's
/// `PROJECT_WRITE_GATE_SQL` — the identical `[20, 15]` call — for writes).
/// Returns the resolved project source; the project id itself is read off
/// that source's row (`_project_id`, `tools.py:174-179`).
pub fn check_scope(
    work_item_id: Option<Uuid>,
    scheduler_binding_id: Option<Uuid>,
    actor: &ScopeActorVerdicts,
    role_ok: bool,
) -> Result<ProjectSource, ToolDenied> {
    if actor.cancel_requested {
        return Err(ToolDenied::new(ToolDenied::RUN_CANCELLED));
    }
    if !actor.creator_active || actor.creator_is_bot || !actor.workspace_member {
        return Err(ToolDenied::new(ToolDenied::ACTOR_NO_LONGER_AUTHORIZED));
    }
    let source = scope_project_source(work_item_id, scheduler_binding_id);
    if !role_ok {
        return Err(ToolDenied::new(ToolDenied::ACTOR_NO_LONGER_AUTHORIZED));
    }
    Ok(source)
}

/// `_scope` run load (`tools.py:47-54`): `AgentRun.objects.select_related(
/// "created_by", "workspace", "work_item", "work_item__project",
/// "scheduler_binding__project", "pod__project").get(pk=run_id)`.
///
/// Django fetches full rows; the narrow column list below is exactly what
/// `_scope` / `_project_id` consume: the run's back-pointers and cancel
/// flag, the creator's active/bot flags, the workspace slug (both
/// membership checks key off it), and each source's project id. Joins
/// follow FK nullability (`work_item`, `scheduler_binding` nullable;
/// `created_by`, `workspace`, `pod` required). `AgentRun` has the plain
/// manager: no `deleted_at` predicate. `.get()` on the PK reads one row
/// (the L3 `LIMIT 1` precedent). `$1` is the run id.
pub const SCOPE_RUN_SQL: &str =
    "SELECT agent_run.id, agent_run.workspace_id, agent_run.created_by_id, \
     agent_run.work_item_id, agent_run.scheduler_binding_id, agent_run.pod_id, \
     agent_run.cancel_requested_at, users.is_active, users.is_bot, workspaces.slug, \
     work_item.project_id, scheduler_binding.project_id, pod.project_id \
     FROM agent_run INNER JOIN users ON users.id = agent_run.created_by_id \
     INNER JOIN workspaces ON workspaces.id = agent_run.workspace_id \
     LEFT OUTER JOIN issues AS work_item ON work_item.id = agent_run.work_item_id \
     LEFT OUTER JOIN scheduler_bindings AS scheduler_binding \
     ON scheduler_binding.id = agent_run.scheduler_binding_id \
     INNER JOIN pod ON pod.id = agent_run.pod_id \
     WHERE agent_run.id = $1 LIMIT 1";

/// `is_workspace_member(run.created_by, run.workspace_id)`
/// (`core/permissions.py:28-34`): an active `WorkspaceMember` row.
/// Soft-delete default manager, hence `deleted_at IS NULL`. `$1` is the
/// user id, `$2` the workspace id (an id, unlike `check_project_role`'s
/// slug).
pub const SCOPE_WORKSPACE_MEMBER_SQL: &str = "SELECT 1 FROM workspace_members \
     WHERE member_id = $1 AND workspace_id = $2 AND is_active AND deleted_at IS NULL LIMIT 1";

/// `check_project_role` with `[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST]`
/// (`core/permissions.py:73-119`) for the read bit: an active
/// `ProjectMember` row with role 20/15/5, OR any-role membership plus
/// workspace-admin (role exactly 20). Same shape as the assistant's
/// `PROJECT_WRITE_GATE_SQL` (`services/src/assistant/tools_issues.rs`,
/// the identical call with `[20, 15]`), which the write bit reuses
/// without forking. `$1` is the user id, `$2` the workspace slug, `$3`
/// the project id. Python filters `workspace__slug`, i.e. a join — the
/// slug binds against `workspaces.slug`, never the id column.
pub const PROJECT_READ_GATE_SQL: &str = "SELECT EXISTS(SELECT 1 FROM project_members \
     INNER JOIN workspaces ON workspaces.id = project_members.workspace_id \
     WHERE project_members.member_id = $1 \
     AND workspaces.slug = $2 \
     AND project_members.project_id = $3 \
     AND project_members.role IN (20, 15, 5) \
     AND project_members.is_active \
     AND project_members.deleted_at IS NULL) \
     OR (EXISTS(SELECT 1 FROM project_members \
     INNER JOIN workspaces ON workspaces.id = project_members.workspace_id \
     WHERE project_members.member_id = $1 \
     AND workspaces.slug = $2 \
     AND project_members.project_id = $3 \
     AND project_members.is_active \
     AND project_members.deleted_at IS NULL) \
     AND EXISTS(SELECT 1 FROM workspace_members \
     INNER JOIN workspaces ON workspaces.id = workspace_members.workspace_id \
     WHERE workspace_members.member_id = $1 \
     AND workspaces.slug = $2 \
     AND workspace_members.role = 20 \
     AND workspace_members.is_active \
     AND workspace_members.deleted_at IS NULL))";

// ---------------------------------------------------------------------------
// Audit (`tools.py:76-157`)
// ---------------------------------------------------------------------------

/// Default ledger `source` (`build_tools`, `tools.py:182`).
pub const SOURCE_INTERNAL: &str = "internal";
/// GitHub-toolset ledger `source` (`github_mcp.py:19`).
pub const SOURCE_MCP: &str = "mcp";
/// GitHub-toolset ledger `server_key` (`github_mcp.py:19`).
pub const SERVER_KEY_GITHUB: &str = "github";

/// `_audit` kill-switch guards (`tools.py:81-88`), in order:
/// `cloud_agent_disabled` → `tool_disabled` → `writes_disabled` (writes
/// only) → `github_tools_disabled` (MCP github-server calls only).
pub fn audit_guards(
    settings: &CloudAgentSettings,
    tool_name: &str,
    risk: ToolRisk,
    source: &str,
    server_key: &str,
) -> Result<(), ToolDenied> {
    if !settings.enabled {
        return Err(ToolDenied::new(ToolDenied::CLOUD_AGENT_DISABLED));
    }
    if settings.disabled_tools.iter().any(|name| name == tool_name) {
        return Err(ToolDenied::new(ToolDenied::TOOL_DISABLED));
    }
    if risk == ToolRisk::Write && !settings.writes_enabled {
        return Err(ToolDenied::new(ToolDenied::WRITES_DISABLED));
    }
    if source == SOURCE_MCP && server_key == SERVER_KEY_GITHUB && !settings.github_tools_enabled {
        return Err(ToolDenied::new(ToolDenied::GITHUB_TOOLS_DISABLED));
    }
    Ok(())
}

/// `current_tool_call_id.get() or str(uuid.uuid4())` (`tools.py:78`): the
/// ambient ContextVar (default `None`, set by the MCP toolset's
/// `process_tool_call`) becomes an explicit parameter — `None` (or `""`,
/// via the `or`) generates a v4 id. The GitHub toolset threads its
/// `tool_call_id` through here.
pub fn resolve_tool_call_id(explicit: Option<&str>) -> String {
    match explicit {
        Some(id) if !id.is_empty() => id.to_owned(),
        _ => Uuid::new_v4().to_string(),
    }
}

/// The ledger columns the replay check consumes
/// (`agent_run_tool_call`, `runner/models.py:1187-1209`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingCall {
    /// `request_fingerprint`.
    pub request_fingerprint: String,
    /// `status`.
    pub status: ToolCallStatus,
    /// `safe_replay_result` (NULL when never succeeded).
    pub safe_replay_result: Option<Value>,
}

/// `_audit` replay outcome (`tools.py:89-95`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayDecision {
    /// No ledger row: run the operation.
    Proceed,
    /// Succeeded row with a stored replay value: return it without running.
    Replay(Value),
}

/// `_audit` idempotency check (`tools.py:89-95`): an existing row under
/// another fingerprint is `tool_call_fingerprint_mismatch`; a succeeded
/// row with a stored replay value returns it; any other existing row is
/// `tool_call_already_submitted`.
pub fn replay_decision(
    existing: Option<&ExistingCall>,
    request_fingerprint: &str,
) -> Result<ReplayDecision, ToolDenied> {
    let Some(row) = existing else {
        return Ok(ReplayDecision::Proceed);
    };
    if row.request_fingerprint != request_fingerprint {
        return Err(ToolDenied::new(ToolDenied::TOOL_CALL_FINGERPRINT_MISMATCH));
    }
    if row.status == ToolCallStatus::Succeeded {
        if let Some(replay) = row.safe_replay_result.clone() {
            return Ok(ReplayDecision::Replay(replay));
        }
    }
    Err(ToolDenied::new(ToolDenied::TOOL_CALL_ALREADY_SUBMITTED))
}

/// `_audit` write admission (`tools.py:99-108`, inside the atomic block
/// after the write-bit scope check): the run's succeeded-write count
/// against `CLOUD_AGENT_WRITE_CALL_LIMIT`, then the single-use rule for
/// tools outside `REPEATABLE_WRITE_TOOLS` (L3).
pub fn write_admission(
    succeeded_write_count: i64,
    tool_already_used: bool,
    tool_name: &str,
    settings: &CloudAgentSettings,
) -> Result<(), ToolDenied> {
    if succeeded_write_count >= settings.write_call_limit {
        return Err(ToolDenied::new(ToolDenied::WRITE_LIMIT));
    }
    if !REPEATABLE_WRITE_TOOLS.contains(&tool_name) && tool_already_used {
        return Err(ToolDenied::new(ToolDenied::WRITE_TOOL_ALREADY_USED));
    }
    Ok(())
}

/// Read-path failure marking (`tools.py:147`):
/// `error_code = type(exc).__name__[:64]` — 64 code points.
pub fn failure_error_code(python_type_name: &str) -> String {
    truncate_chars(python_type_name, ERROR_CODE_MAX_CHARS).to_owned()
}

/// `error_code` cap (`runner/models.py:1200`, `max_length=64`).
pub const ERROR_CODE_MAX_CHARS: usize = 64;

/// `tool_completed` event payload (`tools.py:156`), in Python dict order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolCompletedPayload {
    /// `tool`.
    pub tool: String,
    /// `risk`.
    pub risk: String,
    /// `"succeeded"`.
    pub status: String,
}

/// The terminal event `_audit` appends after a success (`tools.py:156`):
/// `events.append(run_id, "tool_completed", {tool, risk, status:
/// "succeeded"})`. Emission itself is L6 (`events.append`); this pins the
/// kind and payload shape. Failures and denials emit nothing.
pub fn tool_completed_event(
    tool_name: &str,
    risk: ToolRisk,
) -> (&'static str, ToolCompletedPayload) {
    (
        "tool_completed",
        ToolCompletedPayload {
            tool: tool_name.to_owned(),
            risk: risk.as_str().to_owned(),
            status: "succeeded".to_owned(),
        },
    )
}

/// Existing-ledger lookup (`tools.py:89`): `filter(agent_run_id,
/// tool_call_id).first()`. No `Meta.ordering`, so `.first()` orders by
/// PK; the unique constraint makes it single-row regardless. Plain
/// manager: no `deleted_at` predicate. `$1` run id, `$2` call id.
pub const FIND_EXISTING_CALL_SQL: &str = "SELECT status, request_fingerprint, safe_replay_result \
     FROM agent_run_tool_call WHERE agent_run_id = $1 AND tool_call_id = $2 \
     ORDER BY id ASC LIMIT 1";

/// Succeeded-write count (`tools.py:99-101`): `run.tool_calls.filter(
/// risk="write", status=SUCCEEDED).count()`. `$1` run id.
pub const COUNT_SUCCEEDED_WRITES_SQL: &str = "SELECT COUNT(*) FROM agent_run_tool_call \
     WHERE agent_run_id = $1 AND risk = 'write' AND status = 'succeeded'";

/// Single-use probe (`tools.py:104-108`): `run.tool_calls.filter(
/// tool_name, risk="write", status=SUCCEEDED).exists()`. `$1` run id,
/// `$2` tool name.
pub const WRITE_TOOL_USED_SQL: &str = "SELECT 1 FROM agent_run_tool_call \
     WHERE agent_run_id = $1 AND tool_name = $2 AND risk = 'write' AND status = 'succeeded' LIMIT 1";

/// Write-path ledger insert (`tools.py:109-119`): status `prepared`, no
/// `submitted_at` yet. Full column list — Django sends every column
/// with Python-side defaults (`''` has no DB default to fall back on).
/// `$1` id (client-generated, like Django's uuid default), `$2` run,
/// `$3` call id, `$4` source, `$5` server key, `$6` tool, `$7` request
/// fingerprint, `$8` idempotency hash, `$9` `prepared_at` (the caller
/// passes now: `auto_now_add` sets it in Python).
pub const LEDGER_INSERT_WRITE_SQL: &str = "INSERT INTO agent_run_tool_call \
     (id, agent_run_id, tool_call_id, source, server_key, tool_name, risk, status, \
     request_fingerprint, result_fingerprint, idempotency_key_hash, external_operation_id, \
     safe_replay_result, error_code, prepared_at, submitted_at, completed_at) \
     VALUES ($1, $2, $3, $4, $5, $6, 'write', 'prepared', $7, '', $8, '', NULL, '', $9, \
     NULL, NULL)";

/// Read-path ledger insert (`tools.py:132-142`): straight to `submitted`
/// with `submitted_at`; `idempotency_key_hash` keeps its `""` default
/// (the read path never sets it). `$1`-`$7` as the write insert, `$8`
/// `prepared_at`, `$9` `submitted_at`.
pub const LEDGER_INSERT_READ_SQL: &str = "INSERT INTO agent_run_tool_call \
     (id, agent_run_id, tool_call_id, source, server_key, tool_name, risk, status, \
     request_fingerprint, result_fingerprint, idempotency_key_hash, external_operation_id, \
     safe_replay_result, error_code, prepared_at, submitted_at, completed_at) \
     VALUES ($1, $2, $3, $4, $5, $6, 'read', 'submitted', $7, '', '', '', NULL, '', $8, $9, \
     NULL)";

/// Write-path submit flip (`tools.py:120-122`):
/// `save(update_fields=["status", "submitted_at"])`. `$1` ledger id,
/// `$2` now.
pub const LEDGER_MARK_SUBMITTED_SQL: &str =
    "UPDATE agent_run_tool_call SET status = 'submitted', submitted_at = $2 WHERE id = $1";

/// Success marking, both paths (`tools.py:125-129,151-155`):
/// `save(update_fields=["status", "safe_replay_result",
/// "result_fingerprint", "completed_at"])`. `$1` ledger id, `$2` replay
/// value (collapsed past [`REPLAY_COLLAPSE_BYTES`]), `$3` result
/// fingerprint, `$4` now.
pub const LEDGER_MARK_SUCCEEDED_SQL: &str = "UPDATE agent_run_tool_call \
     SET status = 'succeeded', safe_replay_result = $2, result_fingerprint = $3, \
     completed_at = $4 WHERE id = $1";

/// Read-path failure marking (`tools.py:146-149`):
/// `save(update_fields=["status", "error_code", "completed_at"])`.
/// The write path has no equivalent: an op exception leaves the row
/// `submitted`. `$1` ledger id, `$2` [`failure_error_code`], `$3` now.
pub const LEDGER_MARK_FAILED_SQL: &str = "UPDATE agent_run_tool_call \
     SET status = 'failed', error_code = $2, completed_at = $3 WHERE id = $1";

// ---------------------------------------------------------------------------
// Python string semantics (semantic traps)
// ---------------------------------------------------------------------------

/// `value[:max_chars]` (`tools.py:170,209`): Python slicing counts code
/// points and never splits a char. Returns the longest prefix of at most
/// `max_chars` chars.
pub fn truncate_chars(value: &str, max_chars: usize) -> &str {
    match value.char_indices().nth(max_chars) {
        Some((index, _)) => &value[..index],
        None => value,
    }
}

/// Whether `ch` is stripped by `str.strip()` with no args: Unicode
/// whitespace plus U+001C–U+001F, which Python `str.isspace()` accepts
/// but Rust `char::is_whitespace` (the `White_Space` property) does not.
fn is_py_space(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{1f}')
}

/// `value.strip()` (`tools.py:276,347`).
pub fn py_stripped(value: &str) -> &str {
    value.trim_matches(is_py_space)
}

// ---------------------------------------------------------------------------
// Issue shapes (`tools.py:160-171`)
// ---------------------------------------------------------------------------

/// `_issue_data` brief shape (`tools.py:160-171`), in Python dict order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IssueBrief {
    /// `str(issue.id)`.
    pub id: String,
    /// `f"{issue.project.identifier}-{issue.sequence_id}"`.
    pub identifier: String,
    /// `issue.name`.
    pub name: String,
    /// `issue.state.name`, or `None` without a state.
    pub state: Option<String>,
    /// `issue.state.group`, or `None` without a state.
    pub state_group: Option<String>,
    /// `issue.priority`.
    pub priority: String,
}

/// `_issue_data(detail=True)` (`tools.py:169-170`): the brief plus the
/// capped blobs, in Python dict order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IssueDetail {
    /// Brief fields, in brief order.
    pub id: String,
    /// See [`IssueBrief::identifier`].
    pub identifier: String,
    /// See [`IssueBrief::name`].
    pub name: String,
    /// See [`IssueBrief::state`].
    pub state: Option<String>,
    /// See [`IssueBrief::state_group`].
    pub state_group: Option<String>,
    /// See [`IssueBrief::priority`].
    pub priority: String,
    /// `(issue.description_stripped or "")[:20_000]`.
    pub description: String,
    /// `(issue.workpad or "")[:32_000]`.
    pub workpad: String,
}

/// Detail blob caps (`tools.py:170`), in code points.
pub const ISSUE_DESCRIPTION_MAX_CHARS: usize = 20_000;
/// Detail blob caps (`tools.py:170`), in code points.
pub const ISSUE_WORKPAD_MAX_CHARS: usize = 32_000;

/// `_issue_data` (`tools.py:160-171`).
#[allow(clippy::too_many_arguments)]
pub fn issue_data(
    id: &str,
    project_identifier: &str,
    sequence_id: i64,
    name: &str,
    state_name: Option<&str>,
    state_group: Option<&str>,
    priority: &str,
    detail: Option<(&str, &str)>,
) -> IssueDetail {
    let (description_raw, workpad_raw) = detail.unwrap_or(("", ""));
    IssueDetail {
        id: id.to_owned(),
        identifier: format!("{project_identifier}-{sequence_id}"),
        name: name.to_owned(),
        state: state_name.map(str::to_owned),
        state_group: state_group.map(str::to_owned),
        priority: priority.to_owned(),
        description: truncate_chars(description_raw, ISSUE_DESCRIPTION_MAX_CHARS).to_owned(),
        workpad: truncate_chars(workpad_raw, ISSUE_WORKPAD_MAX_CHARS).to_owned(),
    }
}

/// Brief projection of [`issue_data`] (drops the capped blobs, which are
/// empty unless `detail` was passed).
pub fn issue_brief(detail: &IssueDetail) -> IssueBrief {
    IssueBrief {
        id: detail.id.clone(),
        identifier: detail.identifier.clone(),
        name: detail.name.clone(),
        state: detail.state.clone(),
        state_group: detail.state_group.clone(),
        priority: detail.priority.clone(),
    }
}

/// Comment list row (`tools.py:207-211`), in Python dict order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommentRow {
    /// `str(c.id)`.
    pub id: String,
    /// `(c.comment_stripped or "")[:10_000]`.
    pub body: String,
    /// `c.created_at.isoformat()` (the caller formats its timestamptz).
    pub created_at: String,
}

/// Comment body cap (`tools.py:209`), in code points.
pub const COMMENT_BODY_MAX_CHARS: usize = 10_000;

/// Newest-first page size (`tools.py:210`).
pub const COMMENT_LIST_LIMIT: i64 = 50;

/// One comment list row (`tools.py:207-211`).
pub fn comment_row(id: &str, comment_stripped: Option<&str>, created_at_iso: &str) -> CommentRow {
    CommentRow {
        id: id.to_owned(),
        body: truncate_chars(comment_stripped.unwrap_or(""), COMMENT_BODY_MAX_CHARS).to_owned(),
        created_at: created_at_iso.to_owned(),
    }
}

/// Workflow-state row (`tools.py:221-224`), in Python dict order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StateRow {
    /// `str(s.id)`.
    pub id: String,
    /// `s.name`.
    pub name: String,
    /// `s.group`.
    pub group: String,
}

/// Linked code-review row (`tools.py:257-270`), in Python dict order.
/// `number` holds the *string* `external_iid` — the key name is a
/// misnomer, kept verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkedReviewRow {
    /// `str(link.id)`.
    pub id: String,
    /// `link.provider`.
    pub provider: String,
    /// `link.url`.
    pub url: String,
    /// `link.title`.
    pub title: String,
    /// `link.state`.
    pub state: String,
    /// `link.external_iid` (a string).
    pub number: String,
}

/// Linked-review page size (`tools.py:269`).
pub const LINKED_REVIEWS_LIMIT: i64 = 20;

// ---------------------------------------------------------------------------
// Tool argument guards (the closures' pre-audit validation)
// ---------------------------------------------------------------------------

/// Comment length cap (`tools.py:276`), in code points.
pub const COMMENT_MAX_CHARS: usize = 10_000;

/// `pidash_add_current_issue_comment` guard (`tools.py:274-277`):
/// `not body.strip() or len(body) > 10_000` →
/// `ValueError("comment must contain 1-10000 characters")`.
pub fn validate_comment_body(body: &str) -> Result<(), ToolCallFailure> {
    if py_stripped(body).is_empty() || body.chars().count() > COMMENT_MAX_CHARS {
        return Err(ToolCallFailure::InvalidArg(
            "comment must contain 1-10000 characters".to_owned(),
        ));
    }
    Ok(())
}

/// Workpad length cap (`tools.py:298`), in code points.
pub const WORKPAD_MAX_CHARS: usize = 32_000;

/// `pidash_update_current_issue_workpad` guard (`tools.py:296-299`):
/// `len(body) > 32_000` → `ValueError("workpad exceeds 32000
/// characters")`. Empty workpads are allowed (clearing).
pub fn validate_workpad_body(body: &str) -> Result<(), ToolCallFailure> {
    if body.chars().count() > WORKPAD_MAX_CHARS {
        return Err(ToolCallFailure::InvalidArg(
            "workpad exceeds 32000 characters".to_owned(),
        ));
    }
    Ok(())
}

/// Create-issue caps (`tools.py:347`), in code points.
pub const CREATE_TITLE_MAX_CHARS: usize = 255;
/// Create-issue caps (`tools.py:347`), in code points.
pub const CREATE_DESCRIPTION_MAX_CHARS: usize = 20_000;

/// `pidash_create_project_issue` guard (`tools.py:345-348`):
/// `not title.strip() or len(title) > 255 or len(description) > 20_000`
/// → `ValueError("invalid issue title or description length")`.
/// Returns the stripped title (stored as `name`); the audit args record
/// the raw title.
pub fn validate_create_issue(title: &str, description: &str) -> Result<String, ToolCallFailure> {
    if py_stripped(title).is_empty()
        || title.chars().count() > CREATE_TITLE_MAX_CHARS
        || description.chars().count() > CREATE_DESCRIPTION_MAX_CHARS
    {
        return Err(ToolCallFailure::InvalidArg(
            "invalid issue title or description length".to_owned(),
        ));
    }
    Ok(py_stripped(title).to_owned())
}

/// `pidash_search_project_issues` signature default (`tools.py:227`).
pub const SEARCH_DEFAULT_LIMIT: i64 = 20;
/// `pidash_search_project_issues` clamp ceiling (`tools.py:229`).
pub const SEARCH_MAX_LIMIT: i64 = 50;

/// `pidash_search_project_issues` clamp (`tools.py:227-229`):
/// `max(1, min(limit, 50))`. The clamped value feeds both the audit
/// args and the query slice.
pub fn clamp_search_limit(limit: i64) -> i64 {
    limit.clamp(1, SEARCH_MAX_LIMIT)
}

/// Relation fan-out ceiling (`tools.py:396`).
pub const RELATION_MAX_TARGETS: usize = 50;

/// `_relation_write` count check (`tools.py:396-397`), which runs before
/// type validation: `related_issues must list 1-50 issues`. The
/// `isinstance(list)` arm is unrepresentable — callers pass a `Vec`.
pub fn validate_relation_count(count: usize) -> Result<(), ToolCallFailure> {
    if count == 0 || count > RELATION_MAX_TARGETS {
        return Err(ToolCallFailure::InvalidArg(
            "related_issues must list 1-50 issues".to_owned(),
        ));
    }
    Ok(())
}

/// Validated `_relation_write` arguments (`tools.py:393-414`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationWriteArgs {
    /// Source ref (resolved against the run's own project pool).
    pub issue: String,
    /// D-12-validated relation type (the audit args record the
    /// *validated* form: `relation_type` is reassigned before `audit`).
    pub relation_type: String,
    /// Target refs (resolved against the visible pool).
    pub related_issues: Vec<String>,
}

/// `_relation_write` pre-audit validation (`tools.py:396-398`): the
/// count check first, then `relations.validate_relation_type` (D-12,
/// `orchestration/relations.py:81-85`, a `ValueError` whose message
/// passes through). The seam runs only when the count check passed, as
/// in Python.
pub fn relation_write_args<E: std::fmt::Display>(
    issue: &str,
    relation_type: &str,
    related_issues: Vec<String>,
    validate_relation_type: impl FnOnce(&str) -> Result<String, E>,
) -> Result<RelationWriteArgs, ToolCallFailure> {
    validate_relation_count(related_issues.len())?;
    let validated = validate_relation_type(relation_type)
        .map_err(|err| ToolCallFailure::InvalidArg(err.to_string()))?;
    Ok(RelationWriteArgs {
        issue: issue.to_owned(),
        relation_type: validated,
        related_issues,
    })
}

/// `_resolve_relation_refs` miss message (`tools.py:388-391`):
/// `ValueError("issues not found or not accessible: " + ", ".join(
/// unresolved))`. Each item is already `ref or str(raw)` (D-12,
/// `relations.py:160`).
pub fn unresolved_refs_message(unresolved: &[&str]) -> String {
    format!(
        "issues not found or not accessible: {}",
        unresolved.join(", ")
    )
}

/// Where `pidash_list_issue_relations` resolves its source
/// (`tools.py:438-446`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationsSource {
    /// An explicit ref, resolved against the visible pool.
    Ref(String),
    /// The run's bound issue.
    CurrentIssue,
}

/// `pidash_list_issue_relations` source selection (`tools.py:433-446`):
/// an explicit `issue` ref wins; `""` falls back to the run's bound
/// issue; neither is `ValueError("issue is required when the run has no
/// current issue")`.
pub fn relations_source(
    issue_arg: &str,
    work_item_id: Option<Uuid>,
) -> Result<RelationsSource, ToolCallFailure> {
    if !issue_arg.is_empty() {
        Ok(RelationsSource::Ref(issue_arg.to_owned()))
    } else if work_item_id.is_some() {
        Ok(RelationsSource::CurrentIssue)
    } else {
        Err(ToolCallFailure::InvalidArg(
            "issue is required when the run has no current issue".to_owned(),
        ))
    }
}

/// Key under which `_relation_write` attaches the refreshed grouping
/// (`tools.py:406`) and `pidash_list_issue_relations` returns it
/// (`tools.py:446`).
pub const RELATIONS_RESULT_KEY: &str = "relations";

/// `result["relations"] = relations.grouped_relations(source, visible)`
/// (`tools.py:406`): append the D-12 grouping to a relate/unrelate
/// result. Insertion order appends the key last, as in Python.
pub fn with_grouped_relations(mut result: serde_json::Map<String, Value>, grouped: Value) -> Value {
    result.insert(RELATIONS_RESULT_KEY.to_owned(), grouped);
    Value::Object(result)
}

/// `{"issue": identifier, "relations": grouped}`
/// (`tools.py:446`), in Python dict order.
pub fn issue_relations_result(identifier: &str, grouped: Value) -> Value {
    let mut out = serde_json::Map::with_capacity(2);
    out.insert("issue".to_owned(), Value::String(identifier.to_owned()));
    out.insert(RELATIONS_RESULT_KEY.to_owned(), grouped);
    Value::Object(out)
}

/// Repository-path caps (`tools.py:452-460`).
pub const GITHUB_PATH_MAX_CHARS: usize = 1024;
/// Repository-ref caps (`tools.py:461-462`).
pub const GITHUB_REF_MAX_CHARS: usize = 255;

/// `github_get_file` path guard (`tools.py:450-460`): empty, over 1024
/// code points, leading slash, backslash, NUL, or a `..` segment (the
/// `PurePosixPath.parts` check is exactly "some `/`-separated segment
/// is `..`") → `ValueError("invalid relative repository path")`.
pub fn validate_github_path(path: &str) -> Result<(), ToolCallFailure> {
    if path.is_empty()
        || path.chars().count() > GITHUB_PATH_MAX_CHARS
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\x00')
        || path.split('/').any(|segment| segment == "..")
    {
        return Err(ToolCallFailure::InvalidArg(
            "invalid relative repository path".to_owned(),
        ));
    }
    Ok(())
}

/// `github_get_file` ref guard (`tools.py:461-462`): over 255 code
/// points, `://`, or NUL → `ValueError("invalid repository ref")`.
pub fn validate_github_ref(git_ref: &str) -> Result<(), ToolCallFailure> {
    if git_ref.chars().count() > GITHUB_REF_MAX_CHARS
        || git_ref.contains("://")
        || git_ref.contains('\x00')
    {
        return Err(ToolCallFailure::InvalidArg(
            "invalid repository ref".to_owned(),
        ));
    }
    Ok(())
}

/// `github_get_linked_pull_request` aspects (`tools.py:484`), in the
/// source's method-map order.
pub const PR_ASPECTS: [&str; 6] = ["summary", "diff", "files", "checks", "reviews", "comments"];

/// `github_get_linked_pull_request` aspect guard (`tools.py:482-485`):
/// outside the six aspects → `ValueError("invalid pull request aspect")`.
/// Returns the client method the aspect dispatches to
/// (`tools.py:500-508`).
pub fn validate_pr_aspect(aspect: &str) -> Result<&'static str, ToolCallFailure> {
    match aspect {
        "summary" => Ok("get_pull_request"),
        "diff" => Ok("get_pull_request_diff"),
        "files" => Ok("list_pull_request_files"),
        "checks" => Ok("list_pull_request_checks"),
        "reviews" => Ok("list_pull_request_reviews"),
        "comments" => Ok("list_pull_request_comments"),
        _ => Err(ToolCallFailure::InvalidArg(
            "invalid pull request aspect".to_owned(),
        )),
    }
}

/// Default PR aspect (`tools.py:482`).
pub const PR_ASPECT_DEFAULT: &str = "summary";

/// `pidash_transition_current_issue` no-op check (`tools.py:324-325`):
/// the locked row's state is non-NULL and equals the target state.
pub fn transition_same_state(from_state_id: Option<Uuid>, to_state_id: Uuid) -> bool {
    from_state_id == Some(to_state_id)
}

/// `github_get_file` ref default (`tools.py:477`):
/// `ref or repo.default_branch or project.base_branch` — first non-empty
/// wins (`or` treats `""` as missing).
pub fn resolve_file_ref<'a>(
    ref_arg: &'a str,
    repo_default_branch: &'a str,
    project_base_branch: &'a str,
) -> &'a str {
    if !ref_arg.is_empty() {
        ref_arg
    } else if !repo_default_branch.is_empty() {
        repo_default_branch
    } else {
        project_base_branch
    }
}

/// `int(link.external_iid)` (`tools.py:499`): garbage raises the
/// `ValueError` CPython raises (single-quoted literal, as `repr`
/// renders it for typical inputs).
pub fn linked_pr_number(external_iid: &str) -> Result<i64, ToolCallFailure> {
    // `int()` strips exactly the `White_Space` set (probed: U+001C–U+001F
    // are *not* stripped, unlike `str.strip()`), takes an optional sign,
    // and allows single underscores between digits. Non-ASCII decimal
    // digits and out-of-range magnitudes stay rejected (`external_iid`
    // is a GitHub PR number, ASCII digits in practice).
    let text = external_iid.trim();
    let (negative, rest) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let mut digits = String::with_capacity(rest.len());
    for part in rest.split('_') {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid_int(external_iid));
        }
        digits.push_str(part);
    }
    let signed = if negative {
        format!("-{digits}")
    } else {
        digits
    };
    signed.parse::<i64>().map_err(|_| invalid_int(external_iid))
}

/// `int()` failure rendering for base 10 (single-quoted, as `repr`
/// renders typical inputs).
fn invalid_int(external_iid: &str) -> ToolCallFailure {
    ToolCallFailure::InvalidArg(format!(
        "invalid literal for int() with base 10: '{external_iid}'"
    ))
}

// ---------------------------------------------------------------------------
// Tool result shapes (the closures' return values)
// ---------------------------------------------------------------------------

/// `{"updated": ..., "state": ..., "state_id": ...}`
/// (`tools.py:325,341`), in Python dict order — both the same-state
/// no-op and the moved result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TransitionResult {
    /// `updated`.
    pub updated: bool,
    /// `state.name`.
    pub state: String,
    /// `str(state.id)`.
    pub state_id: String,
}

/// `pidash_transition_current_issue` result (`tools.py:325,341`).
pub fn transition_result(updated: bool, state_name: &str, state_id: &str) -> TransitionResult {
    TransitionResult {
        updated,
        state: state_name.to_owned(),
        state_id: state_id.to_owned(),
    }
}

/// `{"created": true, "comment_id": ...}` (`tools.py:292`), in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommentCreated {
    /// Always `true`.
    pub created: bool,
    /// `str(comment.id)`.
    pub comment_id: String,
}

/// `pidash_add_current_issue_comment` result (`tools.py:292`).
pub fn comment_created(comment_id: &str) -> CommentCreated {
    CommentCreated {
        created: true,
        comment_id: comment_id.to_owned(),
    }
}

/// `{"updated": true, "issue_id": ...}` (`tools.py:305`), in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkpadUpdated {
    /// Always `true`.
    pub updated: bool,
    /// `str(run.work_item_id)`.
    pub issue_id: String,
}

/// `pidash_update_current_issue_workpad` result (`tools.py:302-305`).
pub fn workpad_updated(issue_id: &str) -> WorkpadUpdated {
    WorkpadUpdated {
        updated: true,
        issue_id: issue_id.to_owned(),
    }
}

/// `{"created": True, **_issue_data(issue)}` (`tools.py:369`): brief
/// shape (no `detail`), `created` first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IssueCreated {
    /// Always `true`.
    pub created: bool,
    /// See [`IssueBrief::id`].
    pub id: String,
    /// See [`IssueBrief::identifier`].
    pub identifier: String,
    /// See [`IssueBrief::name`].
    pub name: String,
    /// See [`IssueBrief::state`].
    pub state: Option<String>,
    /// See [`IssueBrief::state_group`].
    pub state_group: Option<String>,
    /// See [`IssueBrief::priority`].
    pub priority: String,
}

/// `pidash_create_project_issue` result (`tools.py:369`).
pub fn issue_created(brief: &IssueBrief) -> IssueCreated {
    IssueCreated {
        created: true,
        id: brief.id.clone(),
        identifier: brief.identifier.clone(),
        name: brief.name.clone(),
        state: brief.state.clone(),
        state_group: brief.state_group.clone(),
        priority: brief.priority.clone(),
    }
}

// ---------------------------------------------------------------------------
// Operation SQL (the closures' queries, in `tools.py` order)
// ---------------------------------------------------------------------------

/// `pidash_list_current_issue_comments` (`tools.py:206-212`):
/// `run.work_item.issue_comments.order_by("-created_at")[:50]`.
/// Soft-delete default manager. `$1` is the bound issue id.
pub const CURRENT_ISSUE_COMMENTS_SQL: &str = "SELECT id, comment_stripped, created_at \
     FROM issue_comments WHERE issue_id = $1 AND deleted_at IS NULL \
     ORDER BY created_at DESC LIMIT 50";

/// `pidash_list_project_states` (`tools.py:215-225`):
/// `State.objects.filter(project_id).order_by("sequence")`. The state
/// manager excludes soft-deleted and triage rows
/// (`db/models/state.py:79-84`); `group` stays table-qualified (the
/// assistant precedent). `$1` is the run's project id.
pub const PROJECT_STATES_SQL: &str = "SELECT id, name, states.group FROM states \
     WHERE project_id = $1 AND deleted_at IS NULL AND NOT (states.group = 'triage') \
     ORDER BY sequence ASC";

/// `pidash_search_project_issues` (`tools.py:227-241`):
/// `Issue.objects.filter(project_id).filter(Q(name__icontains=query) |
/// Q(description_stripped__icontains=query)).select_related("project",
/// "state").distinct()[:limit]`. Plain `Issue.objects` (soft-delete
/// only — the triage/archived/draft excludes live on `issue_objects`,
/// which this path does not use). Django compiles `icontains` to
/// `UPPER(col::text) LIKE UPPER(pattern)` — no `ILIKE`, no `ESCAPE`
/// clause (verified with the query compiler; a stable construct). `$1`
/// project id, `$2` the `escape_icontains` output (assistant
/// `tools_issues.rs`, same backslash escaping; `LIKE`'s default escape
/// applies), `$3` the [`clamp_search_limit`] value. No `ORDER BY`:
/// Python slices unordered.
pub const SEARCH_ISSUES_SQL: &str =
    "SELECT DISTINCT issues.id, issues.project_id, issues.state_id, \
     issues.sequence_id, issues.name, issues.priority, states.name, states.group, \
     projects.identifier FROM issues \
     INNER JOIN projects ON projects.id = issues.project_id \
     LEFT OUTER JOIN states ON states.id = issues.state_id \
     WHERE issues.project_id = $1 AND issues.deleted_at IS NULL \
     AND (UPPER(issues.name::text) LIKE UPPER('%' || $2 || '%') \
     OR UPPER(issues.description_stripped::text) LIKE UPPER('%' || $2 || '%')) LIMIT $3";

/// `pidash_get_project_issue` (`tools.py:243-251`):
/// `Issue.objects.select_related("project", "state").get(pk=issue_id,
/// project_id=project_id)`. `$1` issue id, `$2` project id.
pub const GET_PROJECT_ISSUE_SQL: &str = "SELECT issues.id, issues.project_id, issues.state_id, \
     issues.sequence_id, issues.name, issues.priority, issues.description_stripped, \
     issues.workpad, states.name, states.group, projects.identifier FROM issues \
     INNER JOIN projects ON projects.id = issues.project_id \
     LEFT OUTER JOIN states ON states.id = issues.state_id \
     WHERE issues.id = $1 AND issues.project_id = $2 AND issues.deleted_at IS NULL LIMIT 1";

/// `pidash_list_linked_code_reviews` (`tools.py:253-272`):
/// `GitCodeReviewLink.objects.filter(issue_id,
/// deleted_at__isnull=True)[:20]` under `Meta.ordering =
/// ("-created_at",)` (`db/models/integration/git.py:258`). The explicit
/// `deleted_at` filter duplicates the manager's; one predicate suffices.
/// `$1` is the bound issue id.
pub const LINKED_REVIEWS_SQL: &str = "SELECT id, provider, url, title, state, external_iid \
     FROM git_code_review_links WHERE issue_id = $1 AND deleted_at IS NULL \
     ORDER BY created_at DESC LIMIT 20";

/// `pidash_add_current_issue_comment` row (`tools.py:279-292`):
/// `IssueComment.objects.create(issue, project, workspace, actor,
/// comment_html=to_safe_html(body), comment_json={}, speaker_type="agent",
/// speaker_label="Pi Dash Cloud Agent", speaker_agent_run_id=run.id)`.
/// Full column list with the Django-side defaults (`access`,
/// `attachments`, `labels` have no DB default to fall back on). `$1`
/// id, `$2` now (both audit timestamps), `$3` actor (also
/// `created_by_id`: `impersonate(run.created_by)` makes the actor the
/// current user; `updated_by_id` stays NULL on creation per
/// `BaseModel.save`), `$4` workspace, `$5` project, `$6`
/// `comment_stripped` (`strip_tags` of `$7`, Django-side), `$7` safe
/// HTML (assistant-markdown seam), `$8` issue, `$9` run id. The custom
/// `save()` also inserts the `Description` row
/// ([`COMMENT_DESCRIPTION_INSERT_SQL`]) and links it
/// ([`COMMENT_DESCRIPTION_LINK_SQL`]).
pub const COMMENT_INSERT_SQL: &str = "INSERT INTO issue_comments \
     (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, \
     project_id, comment_stripped, comment_json, comment_html, description_id, attachments, \
     labels, issue_id, actor_id, access, external_source, external_id, speaker_type, \
     speaker_label, speaker_agent_run_id, edited_at, parent_id) \
     VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $6, '{}', $7, NULL, '{}', '{}', $8, $3, \
     'INTERNAL', NULL, NULL, 'agent', 'Pi Dash Cloud Agent', $9, NULL, NULL)";

/// Comment `Description` side row (`db/models/issue.py:620-625`):
/// `Description.objects.create(workspace, project, created_by,
/// updated_by, description_stripped, description_json, description_html)`.
/// `$1` id, `$2` now (both audit timestamps), `$3` actor
/// (`created_by_id`; `updated_by_id` NULL on creation), `$4` workspace,
/// `$5` project, `$6` safe HTML, `$7` stripped.
pub const COMMENT_DESCRIPTION_INSERT_SQL: &str = "INSERT INTO descriptions \
     (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, \
     project_id, description_json, description_html, description_binary, description_stripped) \
     VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, '{}', $6, NULL, $7)";

/// Comment↔description link (`db/models/issue.py:623`):
/// `save(update_fields=["description_id"])`. `$1` comment id, `$2`
/// description id.
pub const COMMENT_DESCRIPTION_LINK_SQL: &str =
    "UPDATE issue_comments SET description_id = $2 WHERE id = $1";

/// `pidash_update_current_issue_workpad` (`tools.py:300-307`):
/// `Issue.objects.filter(pk, project_id).update(workpad, updated_at)`.
/// `$1` issue id, `$2` project id, `$3` body, `$4` now.
pub const WORKPAD_UPDATE_SQL: &str =
    "UPDATE issues SET workpad = $3, updated_at = $4 WHERE id = $1 AND project_id = $2 \
     AND deleted_at IS NULL";

/// `pidash_transition_current_issue` target lookup (`tools.py:313`):
/// `State.objects.get(pk=state_id, project_id)`. `$1` state id, `$2`
/// project id.
pub const TRANSITION_STATE_GET_SQL: &str = "SELECT states.id, states.name FROM states \
     WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL \
     AND NOT (states.group = 'triage') LIMIT 1";

/// `pidash_transition_current_issue` lock (`tools.py:322`):
/// `Issue.objects.select_for_update(of=("self",)).select_related("state")
/// .get(pk)` — `FOR UPDATE OF issues` (the nullable state join cannot
/// be locked). No `LIMIT`: `.get()` skips it under `select_for_update`.
/// `$1` is the bound issue id.
pub const TRANSITION_ISSUE_LOCK_SQL: &str = "SELECT issues.id, issues.state_id FROM issues \
     LEFT OUTER JOIN states ON states.id = issues.state_id \
     WHERE issues.id = $1 AND issues.deleted_at IS NULL FOR UPDATE OF issues";

/// `pidash_transition_current_issue` save (`tools.py:334-335`):
/// `locked.save(update_fields=["state", "updated_at", "updated_by"])`
/// under `impersonate(run.created_by)`. `$1` issue, `$2` state, `$3`
/// now, `$4` actor. The caller then routes through
/// `orchestration.handle_issue_state_transition` (D-12 seam) with the
/// run stamped as the mover (`MOVED_BY_RUN_ATTR`, `tools.py:331-340`):
/// the ticker must treat it as an agent move, like the local runner's
/// CLI header.
pub const TRANSITION_ISSUE_SAVE_SQL: &str =
    "UPDATE issues SET state_id = $2, updated_at = $3, updated_by_id = $4 WHERE id = $1";

/// `pidash_create_project_issue` project lock (`tools.py:351`):
/// `Project.objects.select_for_update().get(pk)`. `$1` project id.
pub const CREATE_PROJECT_LOCK_SQL: &str =
    "SELECT id FROM projects WHERE id = $1 AND deleted_at IS NULL FOR UPDATE";

/// `pidash_create_project_issue` default-state probe (`tools.py:352-355`,
/// first arm): `State.objects.filter(project, default=True).first()`
/// under `Meta.ordering = ("sequence",)`. `$1` project id.
pub const CREATE_DEFAULT_STATE_SQL: &str = "SELECT id FROM states WHERE project_id = $1 \
     AND \"default\" AND deleted_at IS NULL AND NOT (states.group = 'triage') \
     ORDER BY sequence ASC LIMIT 1";

/// Backlog-group fallback (`tools.py:354`): `State.objects.filter(
/// project, group="backlog").order_by("sequence").first()`. The triage
/// exclusion is vacuous beside `group = 'backlog'` but Django emits it.
/// `$1` project id.
pub const CREATE_BACKLOG_STATE_SQL: &str = "SELECT id FROM states WHERE project_id = $1 \
     AND states.group = 'backlog' AND deleted_at IS NULL AND NOT (states.group = 'triage') \
     ORDER BY sequence ASC LIMIT 1";

/// `pidash_create_project_issue` sequence probe (`tools.py:356`):
/// `Issue.objects.filter(project).aggregate(Max("sequence_id"))`.
/// Executes — and `Issue.save()` then discards its value (see the
/// header quirks). `$1` project id.
pub const CREATE_SEQ_MAX_SQL: &str =
    "SELECT MAX(sequence_id) FROM issues WHERE project_id = $1 AND deleted_at IS NULL";

/// The tool's `seq` mapping (`tools.py:356`):
/// `(aggregate["value"] or 0) + 1`.
pub fn tool_sequence_id(max_sequence_id: Option<i64>) -> i64 {
    max_sequence_id.unwrap_or(0) + 1
}

/// `Issue.save()` default-state repair (`db/models/issue.py:289-299`,
/// only when the tool's state is `None`): `State.objects.filter(
/// ~Q(is_triage=True), project, default=True).first()` — the explicit
/// boolean filter on top of the manager's group exclusion. `$1`
/// project id.
pub const CREATE_SAVE_DEFAULT_STATE_SQL: &str = "SELECT id FROM states WHERE project_id = $1 \
     AND \"default\" AND NOT is_triage AND deleted_at IS NULL \
     AND NOT (states.group = 'triage') ORDER BY sequence ASC LIMIT 1";

/// `Issue.save()` fallback-state repair (`issue.py:296-297`): no default
/// state exists. `$1` project id.
pub const CREATE_SAVE_FALLBACK_STATE_SQL: &str = "SELECT id FROM states WHERE project_id = $1 \
     AND NOT is_triage AND deleted_at IS NULL AND NOT (states.group = 'triage') \
     ORDER BY sequence ASC LIMIT 1";

/// `Issue.save()` per-project creation lock (`db/models/issue.py:315-320`):
/// `SELECT pg_advisory_xact_lock(%s)` with
/// [`advisory_lock_key`] of the project id. `$1` the lock key.
pub const CREATE_ADVISORY_LOCK_SQL: &str = "SELECT pg_advisory_xact_lock($1)";

/// `convert_uuid_to_integer` (`utils/uuid.py:19-26`): sha256 of the
/// hyphenated UUID, first 8 bytes as signed big-endian.
pub fn advisory_lock_key(project_id: &Uuid) -> i64 {
    let digest = Sha256::digest(project_id.to_string().as_bytes());
    i64::from_be_bytes(digest[..8].try_into().expect("sha256 is 32 bytes"))
}

/// `Issue.save()` effective-sequence scan (`db/models/issue.py:323-327`):
/// `IssueSequence.objects.filter(project).aggregate(Max("sequence"))`.
/// Soft-delete manager only — rows flagged `deleted` still count.
/// `$1` project id.
pub const CREATE_SEQ_SCAN_SQL: &str =
    "SELECT MAX(sequence) FROM issue_sequences WHERE project_id = $1 AND deleted_at IS NULL";

/// The effective `sequence_id` (`issue.py:327`):
/// `last_sequence + 1 if last_sequence else 1`.
pub fn saved_sequence_id(last_sequence: Option<i64>) -> i64 {
    match last_sequence {
        Some(value) if value != 0 => value + 1,
        _ => 1,
    }
}

/// `Issue.save()` sort-order scan (`db/models/issue.py:333-337`):
/// `Issue.objects.filter(project, state).aggregate(Max("sort_order"))`.
/// Stays NULL-bound: `None` keeps the 65535 default. `$1` project id,
/// `$2` state id — when no state resolved at all (a project with zero
/// states), the caller substitutes `state_id IS NULL` for `state_id =
/// $2`, as Django's `filter(state=None)` does.
pub const CREATE_SORT_ORDER_MAX_SQL: &str = "SELECT MAX(sort_order) FROM issues \
     WHERE project_id = $1 AND state_id = $2 AND deleted_at IS NULL";

/// Issue `sort_order` default (`db/models/issue.py:170`).
pub const ISSUE_SORT_ORDER_DEFAULT: f64 = 65535.0;
/// Issue `sort_order` step (`db/models/issue.py:337`).
pub const ISSUE_SORT_ORDER_STEP: f64 = 10000.0;

/// `Issue.save()` sort-order stamping (`issue.py:336-337`): max + 10000
/// when a row exists, else the field default.
pub fn saved_sort_order(largest_sort_order: Option<f64>) -> f64 {
    match largest_sort_order {
        Some(largest) => largest + ISSUE_SORT_ORDER_STEP,
        None => ISSUE_SORT_ORDER_DEFAULT,
    }
}

/// `Issue.save()` assignee-pod default (`db/models/issue.py:277-284` via
/// `Pod.default_for_project_id`, `runner/models.py:174-176`):
/// `Pod.objects.filter(project_id, is_default=True).first()` under
/// `Meta.ordering = ("-is_default", "created_at")` with the
/// soft-delete-excluding `PodManager`. `$1` project id.
pub const CREATE_DEFAULT_POD_SQL: &str = "SELECT id FROM pod WHERE project_id = $1 \
     AND is_default AND deleted_at IS NULL ORDER BY is_default DESC, created_at ASC LIMIT 1";

/// `pidash_create_project_issue` insert (`tools.py:357-368` through
/// `Issue.save`, `db/models/issue.py:267-341`): `Issue.objects.create(
/// workspace, project, state, name=title.strip(),
/// description_html=to_safe_html(description), description_json={},
/// sequence_id=seq, created_by, created_via="cloud_agent")`, then
/// save-derived columns. Full column list with the Django-side defaults
/// (`priority`, `complexity_score`, `git_work_branch`, `workpad` have no
/// DB default to fall back on). `$1` id, `$2`/`$3` now, `$4` creator,
/// `$5` project, `$6` workspace, `$7` state (tool-resolved, else the
/// save repair), `$8` stripped title, `$9` safe HTML
/// (assistant-markdown seam), `$10` `description_stripped`
/// (`strip_tags` of `$9`, NULL when `$9` is empty — Django-side), `$11`
/// the effective [`saved_sequence_id`] (the tool's `seq` is
/// overwritten), `$12` `sort_order` ([`saved_sort_order`]), `$13`
/// `completed_at` (now when a tool-supplied state has group
/// `completed`, else NULL — a save-resolved state is never stamped),
/// `$14` `assigned_pod_id` ([`CREATE_DEFAULT_POD_SQL`], NULL when none).
pub const CREATE_ISSUE_INSERT_SQL: &str = "INSERT INTO issues \
     (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, \
     workspace_id, parent_id, state_id, point, estimate_point_id, name, description_json, \
     description_html, description_stripped, description_binary, priority, complexity_score, \
     start_date, target_date, sequence_id, sort_order, completed_at, archived_at, is_draft, \
     external_source, external_id, type_id, git_work_branch, workpad, created_via, \
     assigned_pod_id, agent_executor) VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, NULL, $7, \
     NULL, NULL, $8, '{}', $9, $10, NULL, 'none', 0, NULL, NULL, $11, $12, $13, NULL, FALSE, \
     NULL, NULL, NULL, '', '', 'cloud_agent', $14, NULL)";

/// `Issue.save()` sequence row (`db/models/issue.py:341`):
/// `IssueSequence.objects.create(issue, sequence, project)` (workspace
/// follows from the project via `ProjectBaseModel.save`; `created_by_id`
/// is the actor — the create runs inside `impersonate`). `$1` id, `$2`
/// now (both audit timestamps), `$3` actor, `$4` project, `$5`
/// workspace, `$6` issue, `$7` the effective sequence.
pub const CREATE_ISSUE_SEQUENCE_INSERT_SQL: &str = "INSERT INTO issue_sequences \
     (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, \
     workspace_id, issue_id, sequence, deleted) \
     VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $6, $7, FALSE)";

/// `_relation_scope` own-project narrowing (`tools.py:379-384`):
/// `visible.filter(project_id=...)`, ANDed onto the visible pool. The
/// pool itself is the assistant's `SCOPED_ISSUES_SQL`
/// (`services/src/assistant/tools_issues.rs`) — the identical
/// `member_project_issues(run.created_by, run.workspace.slug)` call
/// (`core/querysets.py:19-30`), reused without forking. `$3`: the pool
/// binds `$1` (user id) and `$2` (workspace slug).
pub const OWN_PROJECT_ISSUES_PREDICATE: &str = "issues.project_id = $3";

/// `_github_context` binding load (`tools.py:524-526`):
/// `GitRepositoryBinding.objects.select_related("repository",
/// "provider_account__workspace_integration").get(project_id,
/// deleted_at__isnull=True)`. The integration join is dropped: only
/// `account.workspace_integration_id` is consumed, a column on the
/// account row. Unique-per-project, hence one row. `$1` project id.
pub const GITHUB_BINDING_SQL: &str = "SELECT b.id, b.workspace_id, b.repository_id, \
     b.provider_account_id, r.provider, r.host_url, r.namespace, r.name, r.default_branch, \
     a.workspace_id, a.provider, a.host_url, a.auth_type, a.status, a.verified_at, \
     a.workspace_integration_id FROM git_repository_bindings AS b \
     INNER JOIN git_repositories AS r ON r.id = b.repository_id \
     INNER JOIN git_provider_accounts AS a ON a.id = b.provider_account_id \
     WHERE b.project_id = $1 AND b.deleted_at IS NULL LIMIT 1";

/// The binding/account columns `_github_context` verifies
/// (`tools.py:529-539`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubBindingVerdict {
    /// `binding.workspace_id`.
    pub binding_workspace_id: Uuid,
    /// `run.workspace_id`.
    pub run_workspace_id: Uuid,
    /// `binding.repository.provider` / `host_url`.
    pub repo_provider: String,
    /// See [`GithubBindingVerdict::repo_provider`].
    pub repo_host_url: String,
    /// `account.workspace_id`.
    pub account_workspace_id: Uuid,
    /// `account.provider` / `host_url` / `auth_type` / `status`.
    pub account_provider: String,
    /// See [`GithubBindingVerdict::account_provider`].
    pub account_host_url: String,
    /// See [`GithubBindingVerdict::account_provider`].
    pub account_auth_type: String,
    /// See [`GithubBindingVerdict::account_provider`].
    pub account_status: String,
    /// `account.verified_at` is set.
    pub account_verified: bool,
    /// `account.workspace_integration_id`.
    pub account_workspace_integration_id: Option<Uuid>,
}

/// `_github_context` binding verification (`tools.py:528-540`): any
/// mismatch is `ToolDenied("github_binding_unavailable")`. Returns the
/// workspace-integration id for the installation lookup.
pub fn check_github_binding(binding: &GithubBindingVerdict) -> Result<Uuid, ToolDenied> {
    let ok = binding.binding_workspace_id == binding.run_workspace_id
        && binding.repo_provider == "github"
        && binding.repo_host_url == "https://github.com"
        && binding.account_workspace_id == binding.run_workspace_id
        && binding.account_provider == "github"
        && binding.account_host_url == "https://github.com"
        && binding.account_auth_type == "github_app"
        && binding.account_status == "connected"
        && binding.account_verified;
    match (ok, binding.account_workspace_integration_id) {
        (true, Some(integration_id)) => Ok(integration_id),
        _ => Err(ToolDenied::new(ToolDenied::GITHUB_BINDING_UNAVAILABLE)),
    }
}

/// `_github_context` installation lookup (`tools.py:543-547`):
/// `GithubAppInstallation.objects.filter(workspace_integration_id,
/// suspended_at__isnull=True, verified_at__isnull=False).first()` under
/// `Meta.ordering = ("-created_at",)` with the soft-delete default
/// manager. `$1` workspace-integration id.
pub const GITHUB_INSTALLATION_SQL: &str = "SELECT installation_id FROM github_app_installations \
     WHERE workspace_integration_id = $1 AND suspended_at IS NULL AND verified_at IS NOT NULL \
     AND deleted_at IS NULL ORDER BY created_at DESC LIMIT 1";

/// The installation verdict (`tools.py:548-549`): `None` is
/// `ToolDenied("github_installation_unavailable")`, else the id feeds
/// [`GithubClientSpec`].
pub fn check_github_installation(installation_id: Option<i64>) -> Result<i64, ToolDenied> {
    installation_id.ok_or_else(|| ToolDenied::new(ToolDenied::GITHUB_INSTALLATION_UNAVAILABLE))
}

/// `GithubClient.for_installation(installation_id,
/// timeout=CLOUD_AGENT_TOOL_TIMEOUT_SECONDS)` (`tools.py:552-554`,
/// `utils/github_client.py:47-50`): the client is built at execution
/// time; this pins the construction arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GithubClientSpec {
    /// `installation_id`.
    pub installation_id: i64,
    /// `timeout` (`tool_timeout_secs`).
    pub timeout_secs: i64,
}

/// `github_get_linked_pull_request` link lookup (`tools.py:489-496`):
/// `GitCodeReviewLink.objects.filter(issue_id, provider="github",
/// host_url, namespace=repo.namespace, repo_name=repo.name,
/// deleted_at__isnull=True).first()`. `$1` bound issue id, `$2` repo
/// namespace, `$3` repo name. No row is `RuntimeError(
/// "no_linked_pull_request")` ([`MSG_NO_LINKED_PULL_REQUEST`]), not a
/// denial.
pub const LINKED_PR_SQL: &str = "SELECT namespace, repo_name, external_iid \
     FROM git_code_review_links WHERE issue_id = $1 AND provider = 'github' \
     AND host_url = 'https://github.com' AND namespace = $2 AND repo_name = $3 \
     AND deleted_at IS NULL ORDER BY created_at DESC LIMIT 1";

/// Missing-PR-link message (`tools.py:498`).
pub const MSG_NO_LINKED_PULL_REQUEST: &str = "no_linked_pull_request";

/// The PR-call triple (`tools.py:499-508`): `methods[aspect](
/// link.namespace, link.repo_name, number)` — the *link's* coordinates
/// (unlike `get_file`, which uses the repo's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedPrCall {
    /// Client method name (see [`validate_pr_aspect`]).
    pub method: &'static str,
    /// `link.namespace`.
    pub namespace: String,
    /// `link.repo_name`.
    pub repo_name: String,
    /// `int(link.external_iid)`.
    pub number: i64,
}

/// `github_get_linked_pull_request` dispatch (`tools.py:497-508`).
pub fn linked_pr_call(
    aspect: &str,
    namespace: &str,
    repo_name: &str,
    external_iid: &str,
) -> Result<LinkedPrCall, ToolCallFailure> {
    Ok(LinkedPrCall {
        method: validate_pr_aspect(aspect)?,
        namespace: namespace.to_owned(),
        repo_name: repo_name.to_owned(),
        number: linked_pr_number(external_iid)?,
    })
}

/// `github_get_file` client call (`tools.py:464-478`):
/// `client.get_file(repo.namespace, repo.name, path, ref=ref or
/// repo.default_branch or project.base_branch)` — the *repo's*
/// coordinates with the resolved ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetFileCall {
    /// `repo.namespace`.
    pub namespace: String,
    /// `repo.name`.
    pub name: String,
    /// Validated relative path.
    pub path: String,
    /// Resolved ref (see [`resolve_file_ref`]).
    pub git_ref: String,
}

/// `github_get_file` dispatch (`tools.py:450-480`).
pub fn get_file_call(
    path: &str,
    ref_arg: &str,
    repo_namespace: &str,
    repo_name: &str,
    repo_default_branch: &str,
    project_base_branch: &str,
) -> Result<GetFileCall, ToolCallFailure> {
    validate_github_path(path)?;
    validate_github_ref(ref_arg)?;
    Ok(GetFileCall {
        namespace: repo_namespace.to_owned(),
        name: repo_name.to_owned(),
        path: path.to_owned(),
        git_ref: resolve_file_ref(ref_arg, repo_default_branch, project_base_branch).to_owned(),
    })
}

// ---------------------------------------------------------------------------
// GitHub MCP adapter (`github_mcp.py:10-77`)
// ---------------------------------------------------------------------------

/// `build_github_mcp` grant (`github_mcp.py:18`):
/// `sorted(set(allowed_names) & GITHUB_TOOL_NAMES)`.
pub fn build_github_mcp_grant(allowed: &[&str]) -> Vec<&'static str> {
    let mut granted: Vec<&'static str> = allowed
        .iter()
        .filter_map(|name| GITHUB_TOOL_NAMES.iter().find(|tool| *tool == name).copied())
        .collect();
    granted.sort_unstable();
    granted.dedup();
    granted
}

/// `FastMCP` server shape (`github_mcp.py:20-26`): the in-process server
/// closes over one verified run id. `instructions` is `None`, `tasks`
/// off; error masking and strict input validation on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubMcpSpec {
    /// `f"pi-dash-github-{run_id}"`.
    pub server_name: String,
    /// The [`build_github_mcp_grant`] names, each registered with
    /// `readOnlyHint=true, destructiveHint=false, idempotentHint=true`,
    /// `run_in_thread=false`, `timeout=tool_timeout_secs`.
    pub granted: Vec<String>,
    /// `CLOUD_AGENT_TOOL_TIMEOUT_SECONDS`.
    pub tool_timeout_secs: i64,
}

/// `build_github_mcp` (`github_mcp.py:13-52`): the sync tools come from
/// [`build_tools`] with `source="mcp"`, `server_key="github"`; each
/// granted name becomes one async tool closing over its sync twin.
pub fn github_mcp_spec(run_id: &str, allowed: &[&str], tool_timeout_secs: i64) -> GithubMcpSpec {
    GithubMcpSpec {
        server_name: format!("pi-dash-github-{run_id}"),
        granted: build_github_mcp_grant(allowed)
            .iter()
            .map(|name| name.to_string())
            .collect(),
        tool_timeout_secs,
    }
}

/// `MCPToolset` shape (`github_mcp.py:68-77`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubToolsetSpec {
    /// `f"github-{run_id}"`.
    pub id: String,
    /// `include_instructions=False`.
    pub include_instructions: bool,
    /// `cache_resources=False`.
    pub cache_resources: bool,
    /// `cache_prompts=False`.
    pub cache_prompts: bool,
    /// `tool_error_behavior="error"`.
    pub tool_error_behavior: &'static str,
    /// `read_timeout=CLOUD_AGENT_TOOL_TIMEOUT_SECONDS`.
    pub read_timeout_secs: i64,
}

/// `build_github_toolset` (`github_mcp.py:55-77`). `process_tool_call`
/// (`github_mcp.py:59-65`) — set `current_tool_call_id` from the MCP
/// call id, wrap the call in `asyncio.timeout`, reset — becomes
/// explicit plumbing: the caller threads the MCP `tool_call_id` into
/// [`resolve_tool_call_id`] and enforces `read_timeout_secs`
/// (`CLOUD_AGENT_TOOL_TIMEOUT_SECONDS`, the same setting twice).
pub fn github_toolset_spec(run_id: &str, tool_timeout_secs: i64) -> GithubToolsetSpec {
    GithubToolsetSpec {
        id: format!("github-{run_id}"),
        include_instructions: false,
        cache_resources: false,
        cache_prompts: false,
        tool_error_behavior: "error",
        read_timeout_secs: tool_timeout_secs,
    }
}

// ---------------------------------------------------------------------------
// Model routing (`model.py:11-22`, `ee/cloud_agent/model_provider.py:1-7`)
// ---------------------------------------------------------------------------

/// `resolve_model_for_creator` (`model.py:11-22`): delegate to the
/// assistant's EE-overlayable `resolve_model_for_user` seam (CE: BYOK)
/// for `run.created_by`. The creator row arrives as already-read
/// presence bits (the `llm.rs` precedent: this crate holds no DB
/// handle). Raises `AssistantError::LlmConfigMissing` (code
/// `llm_config_missing`) without a usable config, as Python raises
/// `LLMConfigMissing`.
pub fn resolve_model_for_creator(
    has_api_key: bool,
    model_name: &str,
    provider_kind: &str,
    base_url: &str,
    base_url_blocked: bool,
) -> Result<ModelRef, AssistantError> {
    crate::assistant::seams::resolve_model_for_user(
        has_api_key,
        model_name,
        provider_kind,
        base_url,
        base_url_blocked,
    )
}

/// `resolve_model_for_run` (`ee/cloud_agent/model_provider.py:6-7`):
/// the CE passthrough to [`resolve_model_for_creator`].
pub fn resolve_model_for_run(
    has_api_key: bool,
    model_name: &str,
    provider_kind: &str,
    base_url: &str,
    base_url_blocked: bool,
) -> Result<ModelRef, AssistantError> {
    resolve_model_for_creator(
        has_api_key,
        model_name,
        provider_kind,
        base_url,
        base_url_blocked,
    )
}

// ---------------------------------------------------------------------------
// CE extra-toolsets seam (`ee/cloud_agent/toolsets.py:23-50`)
// ---------------------------------------------------------------------------

/// Whether the user opted in to the extra toolsets
/// (`toolsets.py:23-31`). CE: nobody has. Read once at creation and
/// snapshotted onto the plan (L3 takes it as a seam verdict), never
/// consulted at execution time.
pub fn extra_toolsets_enabled_for() -> bool {
    false
}

/// Additional toolsets for the run (`toolsets.py:34-36`). CE: none —
/// generic over the toolset representation so any consumer binds an
/// empty vec. A build overlaying this owns the three documented
/// obligations: admit only what the plan allows (the seam must not
/// widen the closed catalog), degrade-never-fail, report dropped
/// capability on the run's event stream.
pub fn resolve_extra_toolsets_for_run<T>() -> Vec<T> {
    Vec::new()
}

/// Name of the deferred-schema-fetch tool (`toolsets.py:39-50`). CE:
/// none (the prompt section renders only when a name exists).
pub fn extra_toolsets_schema_tool() -> &'static str {
    ""
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/dispatch/fx-disp-05-tools.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn strings(value: &Value) -> Vec<&str> {
        value
            .as_array()
            .expect("array")
            .iter()
            .map(|item| item.as_str().expect("string"))
            .collect()
    }

    /// Membership comparison for the `frozenset` catalogs (`policy.py:34-42`,
    /// `github_mcp.py:10`): Python sets have no order, so the JSON fixture
    /// lists members in recorder order while the Rust consts keep Python
    /// literal order — sort both sides before comparing.
    fn sorted_strings(value: &Value) -> Vec<&str> {
        let mut out = strings(value);
        out.sort_unstable();
        out
    }

    fn sorted_const<const N: usize>(names: [&'static str; N]) -> Vec<&'static str> {
        let mut out = names.to_vec();
        out.sort_unstable();
        out
    }

    fn cloud_settings() -> CloudAgentSettings {
        CloudAgentSettings {
            enabled: true,
            writes_enabled: true,
            github_tools_enabled: true,
            disabled_tools: Vec::new(),
            reconcile_interval_secs: 0,
            model_request_timeout_secs: 0,
            execution_timeout_secs: 0,
            run_soft_limit_secs: 0,
            run_hard_limit_secs: 0,
            stale_grace_secs: 0,
            dispatch_lease_secs: 0,
            dispatch_backoff_secs: 0,
            dispatch_scan_interval_secs: 0,
            sweep_interval_secs: 0,
            dispatch_scan_batch: 0,
            max_queue_age_secs: 0,
            model_request_limit: 0,
            tool_call_limit: 0,
            write_call_limit: 3,
            input_token_limit: 0,
            output_token_limit: 0,
            total_token_limit: 0,
            max_output_tokens_per_request: 0,
            max_queued_per_workspace: 0,
            max_running_per_workspace: 0,
            user_creation_rate_per_minute: 0,
            workspace_creation_rate_per_minute: 0,
            tool_timeout_secs: 60,
            max_tool_result_bytes: 1024,
            max_prompt_bytes: 0,
            max_final_result_bytes: 0,
            max_events: 0,
            block_private_urls: false,
        }
    }

    fn actor() -> ScopeActorVerdicts {
        ScopeActorVerdicts {
            cancel_requested: false,
            creator_active: true,
            creator_is_bot: false,
            workspace_member: true,
        }
    }

    fn github_binding() -> GithubBindingVerdict {
        let workspace = Uuid::nil();
        GithubBindingVerdict {
            binding_workspace_id: workspace,
            run_workspace_id: workspace,
            repo_provider: "github".to_owned(),
            repo_host_url: "https://github.com".to_owned(),
            account_workspace_id: workspace,
            account_provider: "github".to_owned(),
            account_host_url: "https://github.com".to_owned(),
            account_auth_type: "github_app".to_owned(),
            account_status: "connected".to_owned(),
            account_verified: true,
            account_workspace_integration_id: Some(Uuid::nil()),
        }
    }

    #[test]
    fn catalogs_verbatim() {
        let fx = fixture();
        assert_eq!(strings(&fx["read_tools"]), READ_TOOLS);
        assert_eq!(strings(&fx["write_tools"]), WRITE_TOOLS);
        assert_eq!(
            sorted_strings(&fx["repeatable_write_tools"]),
            sorted_const(REPEATABLE_WRITE_TOOLS)
        );
        assert_eq!(
            sorted_strings(&fx["current_issue_write_tools"]),
            sorted_const(crate::dispatch::policy::CURRENT_ISSUE_WRITE_TOOLS)
        );
        assert_eq!(
            sorted_strings(&fx["github_tool_names"]),
            sorted_const(GITHUB_TOOL_NAMES)
        );
        let mut catalog: Vec<&str> = Vec::new();
        catalog.extend(READ_TOOLS);
        catalog.extend(WRITE_TOOLS);
        assert_eq!(catalog, TOOL_CATALOG);
        for name in TOOL_CATALOG {
            assert!(tool_risk(name).is_some(), "{name} has a risk");
        }
        for name in READ_TOOLS {
            assert_eq!(tool_risk(name), Some(ToolRisk::Read));
        }
        for name in WRITE_TOOLS {
            assert_eq!(tool_risk(name), Some(ToolRisk::Write));
        }
        assert_eq!(tool_risk("pidash_nope"), None);
        assert_eq!(tool_risk("_relation_write"), None);
        assert_eq!(ToolRisk::Read.as_str(), "read");
        assert_eq!(ToolRisk::Write.as_str(), "write");
    }

    #[test]
    fn canonical_vectors() {
        let fx = fixture();
        assert_eq!(fx["canonical"]["sorted_keys"], "{\"a\":2,\"b\":1}");
        assert_eq!(fx["canonical"]["separators"], "[1,2,{\"x\":\"y\"}]");
        // Insertion order is hostile on purpose: keys must sort anyway.
        let mut reversed = serde_json::Map::new();
        reversed.insert("b".to_owned(), json!(1));
        reversed.insert("a".to_owned(), json!(2));
        assert_eq!(canonical(&Value::Object(reversed)), br#"{"a":2,"b":1}"#);
        assert_eq!(canonical(&json!([1, 2, {"x": "y"}])), br#"[1,2,{"x":"y"}]"#);
        assert_eq!(
            canonical(&json!({"b": "x", "a": [3, 2]})),
            br#"{"a":[3,2],"b":"x"}"#
        );
        // CPython oracle vectors (probe486.py): ensure_ascii, lowercase
        // hex, surrogate pairs, short escapes, raw `/`.
        assert_eq!(
            canonical(&json!({"k": "café \u{1f600}"})),
            br#"{"k":"caf\u00e9 \ud83d\ude00"}"#
        );
        assert_eq!(
            canonical(&json!({"q": "\"back\\slash/solidus\""})),
            br#"{"q":"\"back\\slash/solidus\""}"#
        );
        assert_eq!(
            canonical(&json!({"n": "line1\nline2\ttab\rcr\x08bel\x0cff"})),
            br#"{"n":"line1\nline2\ttab\rcr\bbel\fff"}"#
        );
        assert_eq!(
            canonical(&json!({"mix": [1, "é", {"z": null, "a": true}]})),
            br#"{"mix":[1,"\u00e9",{"a":true,"z":null}]}"#
        );
        assert_eq!(
            canonical(&json!({"emoji": "\u{1d11e}mus"})),
            br#"{"emoji":"\ud834\udd1emus"}"#
        );
        let ctrls: String = (0x00u32..0x20)
            .chain([0x7f])
            .map(char::from_u32)
            .map(Option::unwrap)
            .collect();
        assert_eq!(
            canonical(&json!({"c": ctrls})),
            br#"{"c":"\u0000\u0001\u0002\u0003\u0004\u0005\u0006\u0007\b\t\n\u000b\f\r\u000e\u000f\u0010\u0011\u0012\u0013\u0014\u0015\u0016\u0017\u0018\u0019\u001a\u001b\u001c\u001d\u001e\u001f\u007f"}"#
        );
    }

    #[test]
    fn fingerprint_vector() {
        let fx = fixture();
        assert_eq!(
            fx["fingerprint"]["sha256_of_canonical"],
            "d3626ac30a87e6f7a6428233b3c68299976865fa5508e4267c5415c76af7a772"
        );
        assert!(fx["fingerprint"]["key_order_stable"].as_bool().unwrap());
        let mut reversed = serde_json::Map::new();
        reversed.insert("b".to_owned(), json!(1));
        reversed.insert("a".to_owned(), json!(2));
        assert_eq!(
            fingerprint(&Value::Object(reversed)),
            "d3626ac30a87e6f7a6428233b3c68299976865fa5508e4267c5415c76af7a772"
        );
        assert_eq!(
            fingerprint(&json!({"a": 2, "b": 1})),
            fingerprint(&json!({"b": 1, "a": 2}))
        );
    }

    #[test]
    fn bounded_vectors() {
        let fx = fixture();
        assert_eq!(fx["bounded"]["ok_passthrough"], json!({"ok": true}));
        assert_eq!(fx["bounded"]["overflow"]["raises"], "RuntimeError");
        assert_eq!(
            fx["bounded"]["overflow"]["message"],
            "tool_result_too_large"
        );
        assert_eq!(
            fx["bounded"]["limit_bytes_setting"],
            "CLOUD_AGENT_MAX_TOOL_RESULT_BYTES"
        );
        assert_eq!(
            bounded(json!({"ok": true}), 1024).unwrap(),
            json!({"ok": true})
        );
        let err = bounded(json!({"ok": true}), 4).unwrap_err();
        assert_eq!(
            err,
            ToolCallFailure::Failed("tool_result_too_large".to_owned())
        );
        assert_eq!(err.python_type_name(), "RuntimeError");
        assert_eq!(MSG_TOOL_RESULT_TOO_LARGE, "tool_result_too_large");
    }

    #[test]
    fn denied_taxonomy() {
        let fx = fixture();
        assert!(fx["audit_shapes"]["tool_denied_is_runtime_error"]
            .as_bool()
            .unwrap());
        assert!(fx["audit_shapes"]["context_var_default_none"]
            .as_bool()
            .unwrap());
        let denied = ToolDenied::new(ToolDenied::RUN_CANCELLED);
        assert_eq!(denied.code(), "run_cancelled");
        assert_eq!(denied.to_string(), "run_cancelled");
        assert_eq!(
            ToolCallFailure::Denied(denied).python_type_name(),
            "ToolDenied"
        );
        assert_eq!(
            ToolCallFailure::InvalidArg("x".to_owned()).python_type_name(),
            "ValueError"
        );
        assert_eq!(
            ToolCallFailure::Failed("x".to_owned()).python_type_name(),
            "RuntimeError"
        );
        assert_eq!(ToolCallFailure::InvalidArg("m".to_owned()).message(), "m");
    }

    #[test]
    fn replay_boundary() {
        let fx = fixture();
        assert_eq!(
            fx["audit_shapes"]["replay_collapsed_value"],
            json!({"ok": true})
        );
        assert!(!collapse_replay(4096));
        assert!(collapse_replay(4097));
        assert!(!collapse_replay(
            fx["audit_shapes"]["replay_keep_under_4096_bytes"]
                .as_u64()
                .unwrap() as usize
        ));
        assert!(collapse_replay(
            fx["audit_shapes"]["replay_collapse_over_4096_bytes"]
                .as_u64()
                .unwrap() as usize
        ));
        assert_eq!(collapsed_replay(), json!({"ok": true}));
        assert_eq!(REPLAY_COLLAPSE_BYTES, 4096);
    }

    #[test]
    fn idempotency_vector() {
        // CPython oracle: sha256 of canonical
        // {"run": "1111...1111", "tool": "pidash_search_project_issues"}.
        assert_eq!(
            idempotency_key_hash(
                "11111111-1111-1111-1111-111111111111",
                "pidash_search_project_issues"
            ),
            "a0d11946ca4cc7fcb6e6e982b2c6d58259a92920bc18421079f542ce162fe14a"
        );
        assert_eq!(
            fixture()["audit_shapes"]["idempotency_key_hash"],
            "sha256 of {\"run\": str(run_id), \"tool\": tool_name}"
        );
    }

    #[test]
    fn write_guards_in_fixture_order() {
        let fx = fixture();
        assert_eq!(
            strings(&fx["audit_shapes"]["write_guards"]),
            [
                "cloud_agent_disabled",
                "tool_disabled",
                "writes_disabled",
                "github_tools_disabled",
                "tool_call_fingerprint_mismatch",
                "tool_call_already_submitted",
                "write_limit",
                "write_tool_already_used",
            ]
        );
        // audit_guards order: disabled → tool → writes → github.
        let mut settings = cloud_settings();
        settings.enabled = false;
        settings.disabled_tools = vec!["pidash_search_project_issues".to_owned()];
        assert_eq!(
            audit_guards(
                &settings,
                "pidash_search_project_issues",
                ToolRisk::Read,
                "internal",
                ""
            ),
            Err(ToolDenied::new(ToolDenied::CLOUD_AGENT_DISABLED))
        );
        settings.enabled = true;
        assert_eq!(
            audit_guards(
                &settings,
                "pidash_search_project_issues",
                ToolRisk::Read,
                "internal",
                ""
            ),
            Err(ToolDenied::new(ToolDenied::TOOL_DISABLED))
        );
        settings.disabled_tools.clear();
        settings.writes_enabled = false;
        assert!(audit_guards(
            &settings,
            "pidash_search_project_issues",
            ToolRisk::Read,
            "internal",
            ""
        )
        .is_ok());
        assert_eq!(
            audit_guards(
                &settings,
                "pidash_add_current_issue_comment",
                ToolRisk::Write,
                "internal",
                ""
            ),
            Err(ToolDenied::new(ToolDenied::WRITES_DISABLED))
        );
        settings.writes_enabled = true;
        settings.github_tools_enabled = false;
        assert_eq!(
            audit_guards(
                &settings,
                "github_get_file",
                ToolRisk::Read,
                "mcp",
                "github"
            ),
            Err(ToolDenied::new(ToolDenied::GITHUB_TOOLS_DISABLED))
        );
        assert!(audit_guards(&settings, "github_get_file", ToolRisk::Read, "internal", "").is_ok());
        assert!(audit_guards(&settings, "github_get_file", ToolRisk::Read, "mcp", "other").is_ok());
        settings.github_tools_enabled = true;
        assert!(audit_guards(
            &settings,
            "github_get_file",
            ToolRisk::Read,
            "mcp",
            "github"
        )
        .is_ok());
        // replay_decision order: mismatch → replay-or-submitted.
        let row = ExistingCall {
            request_fingerprint: "fp".to_owned(),
            status: ToolCallStatus::Succeeded,
            safe_replay_result: Some(json!({"ok": true})),
        };
        assert_eq!(
            replay_decision(Some(&row), "other"),
            Err(ToolDenied::new(ToolDenied::TOOL_CALL_FINGERPRINT_MISMATCH))
        );
        assert_eq!(
            replay_decision(Some(&row), "fp"),
            Ok(ReplayDecision::Replay(json!({"ok": true})))
        );
        assert_eq!(replay_decision(None, "fp"), Ok(ReplayDecision::Proceed));
        let submitted = ExistingCall {
            status: ToolCallStatus::Submitted,
            ..row.clone()
        };
        assert_eq!(
            replay_decision(Some(&submitted), "fp"),
            Err(ToolDenied::new(ToolDenied::TOOL_CALL_ALREADY_SUBMITTED))
        );
        let bare = ExistingCall {
            safe_replay_result: None,
            ..row
        };
        assert_eq!(
            replay_decision(Some(&bare), "fp"),
            Err(ToolDenied::new(ToolDenied::TOOL_CALL_ALREADY_SUBMITTED))
        );
        // write_admission order: limit → single-use.
        let settings = cloud_settings();
        assert_eq!(
            write_admission(3, false, "pidash_add_current_issue_comment", &settings),
            Err(ToolDenied::new(ToolDenied::WRITE_LIMIT))
        );
        assert_eq!(
            write_admission(2, true, "pidash_add_current_issue_comment", &settings),
            Err(ToolDenied::new(ToolDenied::WRITE_TOOL_ALREADY_USED))
        );
        assert!(write_admission(2, true, "pidash_relate_issues", &settings).is_ok());
        assert!(write_admission(2, false, "pidash_add_current_issue_comment", &settings).is_ok());
    }

    #[test]
    fn read_failure_marks() {
        assert_eq!(
            fixture()["audit_shapes"]["read_failure_marks"],
            "FAILED + error_code=type(exc).__name__[:64]"
        );
        assert_eq!(failure_error_code("ValueError"), "ValueError");
        let long = "E".repeat(70);
        assert_eq!(failure_error_code(&long), "E".repeat(64));
        let wide = "é".repeat(70);
        assert_eq!(failure_error_code(&wide).chars().count(), 64);
        assert_eq!(ERROR_CODE_MAX_CHARS, 64);
    }

    #[test]
    fn terminal_event_shape() {
        assert_eq!(
            fixture()["audit_shapes"]["terminal_event"],
            "events.append(run_id, \"tool_completed\", {tool, risk, status: succeeded})"
        );
        let (kind, payload) = tool_completed_event("pidash_search_project_issues", ToolRisk::Read);
        assert_eq!(kind, "tool_completed");
        assert_eq!(
            serde_json::to_string(&payload).unwrap(),
            r#"{"tool":"pidash_search_project_issues","risk":"read","status":"succeeded"}"#
        );
    }

    #[test]
    fn scope_vectors() {
        let fx = fixture();
        assert_eq!(
            fx["scope_shape"]["cancelled"],
            "run_cancel_requested_at set -> ToolDenied(run_cancelled)"
        );
        assert_eq!(
            fx["scope_shape"]["actor"],
            "created_by inactive/bot/non-member -> ToolDenied(actor_no_longer_authorized)"
        );
        assert_eq!(
            fx["scope_shape"]["roles"],
            "write needs ADMIN/MEMBER; read also allows GUEST"
        );
        assert_eq!(
            fx["scope_shape"]["project_resolution"],
            "work_item.project / scheduler_binding.project / pod.project"
        );
        assert_eq!(scope_roles(true), &[20, 15]);
        assert_eq!(scope_roles(false), &[20, 15, 5]);
        assert_eq!(SCOPE_ROLES_WRITE, [ROLE_ADMIN, ROLE_MEMBER]);
        assert_eq!(SCOPE_ROLES_READ, [ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST]);
        let run = Uuid::nil();
        assert_eq!(
            check_scope(Some(run), None, &actor(), true),
            Ok(ProjectSource::WorkItem)
        );
        assert_eq!(
            check_scope(None, Some(run), &actor(), true),
            Ok(ProjectSource::SchedulerBinding)
        );
        assert_eq!(
            check_scope(None, None, &actor(), true),
            Ok(ProjectSource::Pod)
        );
        let cancelled = ScopeActorVerdicts {
            cancel_requested: true,
            ..actor()
        };
        assert_eq!(
            check_scope(Some(run), None, &cancelled, true),
            Err(ToolDenied::new(ToolDenied::RUN_CANCELLED))
        );
        for actor in [
            ScopeActorVerdicts {
                creator_active: false,
                ..actor()
            },
            ScopeActorVerdicts {
                creator_is_bot: true,
                ..actor()
            },
            ScopeActorVerdicts {
                workspace_member: false,
                ..actor()
            },
        ] {
            assert_eq!(
                check_scope(Some(run), None, &actor, true),
                Err(ToolDenied::new(ToolDenied::ACTOR_NO_LONGER_AUTHORIZED))
            );
        }
        assert_eq!(
            check_scope(Some(run), None, &actor(), false),
            Err(ToolDenied::new(ToolDenied::ACTOR_NO_LONGER_AUTHORIZED))
        );
    }

    #[test]
    fn tool_call_id_resolution() {
        assert_eq!(resolve_tool_call_id(Some("abc")), "abc");
        for generated in [resolve_tool_call_id(None), resolve_tool_call_id(Some(""))] {
            let parsed = Uuid::parse_str(&generated).expect("v4 uuid");
            assert_eq!(parsed.get_version(), Some(uuid::Version::Random));
        }
    }

    #[test]
    fn issue_data_shapes() {
        let fx = fixture();
        assert_eq!(
            fx["issue_data"]["detail_caps"],
            "description[:20000], workpad[:32000]; comments newest 50, body[:10000], isoformat timestamps"
        );
        let detail = issue_data(
            "11111111-1111-1111-1111-111111111111",
            "PRJ",
            42,
            "Fix it",
            Some("In Progress"),
            Some("started"),
            "high",
            None,
        );
        let brief = issue_brief(&detail);
        assert_eq!(
            serde_json::to_value(&brief).unwrap(),
            fx["issue_data"]["brief"]
        );
        assert_eq!(
            serde_json::to_string(&brief).unwrap(),
            r#"{"id":"11111111-1111-1111-1111-111111111111","identifier":"PRJ-42","name":"Fix it","state":"In Progress","state_group":"started","priority":"high"}"#
        );
        let mut keys: Vec<&str> = strings(&fx["issue_data"]["detail_keys"]);
        keys.sort_unstable();
        let mut ours = vec![
            "id",
            "identifier",
            "name",
            "state",
            "state_group",
            "priority",
            "description",
            "workpad",
        ];
        ours.sort_unstable();
        assert_eq!(keys, ours);
        let stateless = issue_data("id", "PRJ", 1, "n", None, None, "none", None);
        assert_eq!(stateless.state, None);
        assert_eq!(stateless.state_group, None);
        // Caps count code points and never split a char.
        let wide = "é".repeat(20_001);
        let capped = issue_data("id", "PRJ", 1, "n", None, None, "none", Some((&wide, "w")));
        assert_eq!(capped.description.chars().count(), 20_000);
        assert!(capped
            .description
            .is_char_boundary(capped.description.len()));
        let wide_pad = "𝄞".repeat(32_001);
        let capped_pad = issue_data(
            "id",
            "PRJ",
            1,
            "n",
            None,
            None,
            "none",
            Some(("d", &wide_pad)),
        );
        assert_eq!(capped_pad.workpad.chars().count(), 32_000);
        assert_eq!(COMMENT_LIST_LIMIT, 50);
        let row = comment_row("c", Some(&"b".repeat(10_001)), "2026-09-30T00:00:00+00:00");
        assert_eq!(row.body.chars().count(), 10_000);
        assert_eq!(comment_row("c", None, "t").body, "");
        assert_eq!(
            serde_json::to_string(&comment_row("c", Some("b"), "t")).unwrap(),
            r#"{"id":"c","body":"b","created_at":"t"}"#
        );
        assert_eq!(
            serde_json::to_string(&StateRow {
                id: "s".to_owned(),
                name: "n".to_owned(),
                group: "g".to_owned()
            })
            .unwrap(),
            r#"{"id":"s","name":"n","group":"g"}"#
        );
        // "number" is the string external_iid, kept verbatim.
        let review = LinkedReviewRow {
            id: "l".to_owned(),
            provider: "github".to_owned(),
            url: "u".to_owned(),
            title: "t".to_owned(),
            state: "open".to_owned(),
            number: "12".to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&review).unwrap(),
            r#"{"id":"l","provider":"github","url":"u","title":"t","state":"open","number":"12"}"#
        );
        assert_eq!(LINKED_REVIEWS_LIMIT, 20);
    }

    #[test]
    fn guard_predicates() {
        let fx = fixture();
        let guards = &fx["guard_predicates"];
        assert!(guards["github_path"]
            .as_str()
            .unwrap()
            .contains("tools.py:452-460"));
        let long_path = "p".repeat(1025);
        for bad in [
            "",
            long_path.as_str(),
            "/abs",
            "a\\b",
            "a\x00b",
            "a/../b",
            "../x",
            "a/..",
        ] {
            assert_eq!(
                validate_github_path(bad),
                Err(ToolCallFailure::InvalidArg(
                    "invalid relative repository path".to_owned()
                )),
                "{bad:?} rejected"
            );
        }
        let max_path = "p".repeat(1024);
        for ok in ["a/b", "a..b", "...", ".", "x", max_path.as_str()] {
            assert!(validate_github_path(ok).is_ok(), "{ok:?} accepted");
        }
        assert!(guards["github_ref"]
            .as_str()
            .unwrap()
            .contains("tools.py:461-462"));
        let long_ref = "r".repeat(256);
        for bad in [long_ref.as_str(), "https://x", "a\x00"] {
            assert_eq!(
                validate_github_ref(bad),
                Err(ToolCallFailure::InvalidArg(
                    "invalid repository ref".to_owned()
                )),
                "{bad:?} rejected"
            );
        }
        assert!(validate_github_ref("").is_ok());
        assert!(validate_github_ref("main").is_ok());
        assert_eq!(strings(&guards["github_pr_aspects"]), PR_ASPECTS);
        for (aspect, method) in [
            ("summary", "get_pull_request"),
            ("diff", "get_pull_request_diff"),
            ("files", "list_pull_request_files"),
            ("checks", "list_pull_request_checks"),
            ("reviews", "list_pull_request_reviews"),
            ("comments", "list_pull_request_comments"),
        ] {
            assert_eq!(validate_pr_aspect(aspect), Ok(method));
        }
        assert_eq!(
            validate_pr_aspect("bogus"),
            Err(ToolCallFailure::InvalidArg(
                "invalid pull request aspect".to_owned()
            ))
        );
        assert_eq!(PR_ASPECT_DEFAULT, "summary");
        assert!(guards["comment"]
            .as_str()
            .unwrap()
            .contains("tools.py:276-277"));
        let long_comment = "x".repeat(10_001);
        for bad in ["", "   ", "\u{1c}\u{1d}", long_comment.as_str()] {
            assert_eq!(
                validate_comment_body(bad),
                Err(ToolCallFailure::InvalidArg(
                    "comment must contain 1-10000 characters".to_owned()
                )),
                "{bad:?} rejected"
            );
        }
        assert!(validate_comment_body(&"é".repeat(10_000)).is_ok());
        assert!(validate_comment_body("hi").is_ok());
        assert!(guards["workpad"]
            .as_str()
            .unwrap()
            .contains("tools.py:298-299"));
        assert!(validate_workpad_body("").is_ok());
        assert!(validate_workpad_body(&"w".repeat(32_000)).is_ok());
        assert_eq!(
            validate_workpad_body(&"w".repeat(32_001)),
            Err(ToolCallFailure::InvalidArg(
                "workpad exceeds 32000 characters".to_owned()
            ))
        );
        assert!(guards["create_issue"]
            .as_str()
            .unwrap()
            .contains("tools.py:347-348"));
        assert_eq!(
            validate_create_issue("  \u{1c} ", "d"),
            Err(ToolCallFailure::InvalidArg(
                "invalid issue title or description length".to_owned()
            ))
        );
        assert!(validate_create_issue(&"t".repeat(255), "d").is_ok());
        assert!(validate_create_issue("t", &"d".repeat(20_000)).is_ok());
        assert!(validate_create_issue(&"t".repeat(256), "d").is_err());
        assert!(validate_create_issue("t", &"d".repeat(20_001)).is_err());
        assert_eq!(
            validate_create_issue("  Title \u{1c}", "d").unwrap(),
            "Title"
        );
        assert!(guards["search_limit_clamp"]
            .as_str()
            .unwrap()
            .contains("max(1, min(limit, 50))"));
        assert_eq!(clamp_search_limit(-5), 1);
        assert_eq!(clamp_search_limit(0), 1);
        assert_eq!(clamp_search_limit(20), 20);
        assert_eq!(clamp_search_limit(50), 50);
        assert_eq!(clamp_search_limit(51), 50);
        assert_eq!(SEARCH_DEFAULT_LIMIT, 20);
        assert!(guards["relation_count"]
            .as_str()
            .unwrap()
            .contains("tools.py:396"));
        assert!(validate_relation_count(0).is_err());
        assert!(validate_relation_count(1).is_ok());
        assert!(validate_relation_count(50).is_ok());
        assert!(validate_relation_count(51).is_err());
        assert_eq!(
            validate_relation_count(0).unwrap_err().message(),
            "related_issues must list 1-50 issues"
        );
        assert!(guards["transition_same_state"]
            .as_str()
            .unwrap()
            .contains("tools.py:324-325"));
        let state = Uuid::nil();
        assert!(transition_same_state(Some(state), state));
        assert!(!transition_same_state(None, state));
        assert!(!transition_same_state(Some(Uuid::max()), state));
    }

    #[test]
    fn py_strip_matches_python() {
        // CPython oracle (probe486b.py): exactly these 29 chars strip.
        let stripped = [
            0x09u32, 0x0a, 0x0b, 0x0c, 0x0d, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x85, 0xa0, 0x1680,
            0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200a,
            0x2028, 0x2029, 0x202f, 0x205f, 0x3000,
        ];
        assert_eq!(stripped.len(), 29);
        for cp in 0u32..0x110000 {
            let Some(ch) = char::from_u32(cp) else {
                continue;
            };
            assert_eq!(
                py_stripped(&ch.to_string()).is_empty(),
                stripped.contains(&cp),
                "U+{cp:04X}"
            );
        }
        assert_eq!(py_stripped("  padded  "), "padded");
        assert_eq!(py_stripped("\u{1c}hi\u{1f}"), "hi");
        assert_eq!(py_stripped("\u{0}no-strip\u{0}"), "\u{0}no-strip\u{0}");
    }

    #[test]
    fn build_tools_gating() {
        let fx = fixture();
        assert_eq!(
            strings(&fx["build_tools_gating"]["granted_only"]),
            ["pidash_get_current_issue"]
        );
        assert!(fx["build_tools_gating"]["sorted_by_name"]
            .as_bool()
            .unwrap());
        assert_eq!(
            build_tools(&["pidash_get_current_issue", "pidash_nope"]),
            ["pidash_get_current_issue"]
        );
        assert_eq!(
            build_tools(&[
                "pidash_search_project_issues",
                "github_get_file",
                "pidash_get_current_issue"
            ]),
            [
                "github_get_file",
                "pidash_get_current_issue",
                "pidash_search_project_issues"
            ]
        );
        // Duplicates preserved, like the comprehension; helpers ungrantable.
        assert_eq!(
            build_tools(&["github_get_file", "github_get_file"]).len(),
            2
        );
        assert!(build_tools(&["_relation_write", "audit", "granted"]).is_empty());
        assert!(build_tools(&[]).is_empty());
        let all: Vec<&str> = TOOL_CATALOG.to_vec();
        assert_eq!(build_tools(&all).len(), 15);
    }

    #[test]
    fn github_context_denials() {
        let fx = fixture();
        assert_eq!(
            strings(&fx["github_context_denials"]),
            [
                "github_binding_unavailable",
                "github_installation_unavailable"
            ]
        );
        assert_eq!(check_github_binding(&github_binding()), Ok(Uuid::nil()));
        let denied = Err::<Uuid, _>(ToolDenied::new(ToolDenied::GITHUB_BINDING_UNAVAILABLE));
        let mut binding = github_binding();
        binding.binding_workspace_id = Uuid::max();
        assert_eq!(check_github_binding(&binding), denied);
        binding = github_binding();
        binding.repo_provider = "gitlab".to_owned();
        assert_eq!(check_github_binding(&binding), denied);
        binding = github_binding();
        binding.repo_host_url = "https://ghe.example.com".to_owned();
        assert_eq!(check_github_binding(&binding), denied);
        binding = github_binding();
        binding.account_workspace_id = Uuid::max();
        assert_eq!(check_github_binding(&binding), denied);
        binding = github_binding();
        binding.account_provider = "gitlab".to_owned();
        assert_eq!(check_github_binding(&binding), denied);
        binding = github_binding();
        binding.account_host_url = "https://ghe.example.com".to_owned();
        assert_eq!(check_github_binding(&binding), denied);
        binding = github_binding();
        binding.account_auth_type = "pat".to_owned();
        assert_eq!(check_github_binding(&binding), denied);
        binding = github_binding();
        binding.account_status = "revoked".to_owned();
        assert_eq!(check_github_binding(&binding), denied);
        binding = github_binding();
        binding.account_verified = false;
        assert_eq!(check_github_binding(&binding), denied);
        binding = github_binding();
        binding.account_workspace_integration_id = None;
        assert_eq!(check_github_binding(&binding), denied);
        assert_eq!(
            check_github_installation(None),
            Err(ToolDenied::new(ToolDenied::GITHUB_INSTALLATION_UNAVAILABLE))
        );
        assert_eq!(check_github_installation(Some(7)), Ok(7));
    }

    #[test]
    fn github_dispatch_shapes() {
        assert_eq!(resolve_file_ref("r", "d", "b"), "r");
        assert_eq!(resolve_file_ref("", "d", "b"), "d");
        assert_eq!(resolve_file_ref("", "", "b"), "b");
        assert_eq!(resolve_file_ref("", "", ""), "");
        let call = get_file_call("a/b.md", "", "ns", "repo", "dev", "main").unwrap();
        assert_eq!(call.namespace, "ns");
        assert_eq!(call.name, "repo");
        assert_eq!(call.git_ref, "dev");
        assert!(get_file_call("../x", "", "ns", "repo", "d", "b").is_err());
        let pr = linked_pr_call("diff", "ns", "repo", "12").unwrap();
        assert_eq!(pr.method, "get_pull_request_diff");
        assert_eq!(
            (pr.namespace.as_str(), pr.repo_name.as_str(), pr.number),
            ("ns", "repo", 12)
        );
        assert_eq!(
            linked_pr_call("bogus", "ns", "repo", "12")
                .unwrap_err()
                .message(),
            "invalid pull request aspect"
        );
        assert_eq!(MSG_NO_LINKED_PULL_REQUEST, "no_linked_pull_request");
        assert_eq!(linked_pr_number("42"), Ok(42));
        assert_eq!(linked_pr_number("  42  "), Ok(42));
        assert_eq!(linked_pr_number("+7"), Ok(7));
        assert_eq!(linked_pr_number("1_2"), Ok(12));
        assert_eq!(linked_pr_number("-0"), Ok(0));
        for bad in ["abc-12", "", "1__2", "_1", "1_", "+_1", "--1", "\u{1c}42"] {
            let err = linked_pr_number(bad).unwrap_err();
            assert_eq!(err.python_type_name(), "ValueError");
            assert_eq!(
                err.message(),
                format!("invalid literal for int() with base 10: '{bad}'")
            );
        }
    }

    #[test]
    fn tool_result_bytes() {
        assert_eq!(
            serde_json::to_string(&transition_result(false, "Backlog", "s")).unwrap(),
            r#"{"updated":false,"state":"Backlog","state_id":"s"}"#
        );
        assert_eq!(
            serde_json::to_string(&transition_result(true, "Done", "s")).unwrap(),
            r#"{"updated":true,"state":"Done","state_id":"s"}"#
        );
        assert_eq!(
            serde_json::to_string(&comment_created("c")).unwrap(),
            r#"{"created":true,"comment_id":"c"}"#
        );
        assert_eq!(
            serde_json::to_string(&workpad_updated("i")).unwrap(),
            r#"{"updated":true,"issue_id":"i"}"#
        );
        let brief = issue_brief(&issue_data(
            "i",
            "PRJ",
            3,
            "n",
            Some("s"),
            Some("g"),
            "low",
            None,
        ));
        assert_eq!(
            serde_json::to_string(&issue_created(&brief)).unwrap(),
            r#"{"created":true,"id":"i","identifier":"PRJ-3","name":"n","state":"s","state_group":"g","priority":"low"}"#
        );
        let mut result = serde_json::Map::new();
        result.insert("created".to_owned(), json!(["PRJ-4"]));
        assert_eq!(
            with_grouped_relations(result, json!({"blocked_by": []})).to_string(),
            r#"{"created":["PRJ-4"],"relations":{"blocked_by":[]}}"#
        );
        assert_eq!(
            issue_relations_result("PRJ-3", json!({})).to_string(),
            r#"{"issue":"PRJ-3","relations":{}}"#
        );
        assert_eq!(RELATIONS_RESULT_KEY, "relations");
    }

    #[test]
    fn relation_args_flow() {
        // Count check runs before the seam: the seam never fires here.
        let err = relation_write_args(
            "PRJ-1",
            "blocked_by",
            vec![],
            |_: &str| -> Result<String, String> { panic!("seam must not run") },
        )
        .unwrap_err();
        assert_eq!(err.message(), "related_issues must list 1-50 issues");
        let args = relation_write_args("PRJ-1", " BLOCKED_BY ", vec!["PRJ-2".to_owned()], |raw| {
            Ok::<_, String>(raw.trim().to_lowercase())
        })
        .unwrap();
        assert_eq!(args.issue, "PRJ-1");
        assert_eq!(args.relation_type, "blocked_by");
        assert_eq!(args.related_issues, ["PRJ-2"]);
        let err = relation_write_args("PRJ-1", "bogus", vec!["PRJ-2".to_owned()], |_| {
            Err::<String, _>("relation_type must be one of: blocked_by, ...".to_owned())
        })
        .unwrap_err();
        assert_eq!(err.python_type_name(), "ValueError");
        assert_eq!(
            unresolved_refs_message(&["PRJ-9", "nope"]),
            "issues not found or not accessible: PRJ-9, nope"
        );
        assert_eq!(
            relations_source("PRJ-1", None),
            Ok(RelationsSource::Ref("PRJ-1".to_owned()))
        );
        assert_eq!(
            relations_source("", Some(Uuid::nil())),
            Ok(RelationsSource::CurrentIssue)
        );
        assert_eq!(
            relations_source("", None).unwrap_err().message(),
            "issue is required when the run has no current issue"
        );
    }

    #[test]
    fn model_routing() {
        let fx = fixture();
        assert!(fx["resolve_model_routing"]["resolve_model_for_creator"]
            .as_str()
            .unwrap()
            .contains("resolve_model_for_user"));
        assert!(fx["resolve_model_routing"]["resolve_model_for_run"]
            .as_str()
            .unwrap()
            .contains("resolve_model_for_creator"));
        let err = resolve_model_for_creator(false, "m", "anthropic", "", false).unwrap_err();
        assert_eq!(err.code(), "llm_config_missing");
        assert_eq!(
            resolve_model_for_run(false, "m", "anthropic", "", false)
                .unwrap_err()
                .code(),
            "llm_config_missing"
        );
        assert_eq!(
            resolve_model_for_creator(true, "claude-x", "anthropic", "", false),
            resolve_model_for_run(true, "claude-x", "anthropic", "", false)
        );
        assert!(resolve_model_for_creator(true, "gpt", "openai", "https://x", false).is_ok());
        assert_eq!(
            resolve_model_for_creator(true, "gpt", "openai", "https://x", true)
                .unwrap_err()
                .code(),
            "llm_config_missing"
        );
    }

    #[test]
    fn ce_seam_defaults() {
        let fx = fixture();
        assert!(!fx["ce_seam_defaults"]["extra_toolsets_enabled_for"]
            .as_bool()
            .unwrap());
        assert_eq!(
            fx["ce_seam_defaults"]["resolve_extra_toolsets_for_run"],
            json!([])
        );
        assert_eq!(fx["ce_seam_defaults"]["extra_toolsets_schema_tool"], "");
        assert!(!extra_toolsets_enabled_for());
        let none: Vec<()> = resolve_extra_toolsets_for_run();
        assert!(none.is_empty());
        assert_eq!(extra_toolsets_schema_tool(), "");
    }

    #[test]
    fn github_toolset_specs() {
        assert_eq!(
            build_github_mcp_grant(&[
                "github_get_file",
                "pidash_nope",
                "github_get_linked_pull_request",
                "github_get_file"
            ]),
            ["github_get_file", "github_get_linked_pull_request"]
        );
        assert!(build_github_mcp_grant(&["pidash_get_current_issue"]).is_empty());
        let mcp = github_mcp_spec("run-1", &["github_get_file"], 60);
        assert_eq!(mcp.server_name, "pi-dash-github-run-1");
        assert_eq!(mcp.granted, ["github_get_file"]);
        assert_eq!(mcp.tool_timeout_secs, 60);
        let toolset = github_toolset_spec("run-1", 60);
        assert_eq!(toolset.id, "github-run-1");
        assert!(!toolset.include_instructions);
        assert!(!toolset.cache_resources);
        assert!(!toolset.cache_prompts);
        assert_eq!(toolset.tool_error_behavior, "error");
        assert_eq!(toolset.read_timeout_secs, 60);
        assert_eq!(SOURCE_INTERNAL, "internal");
        assert_eq!(SOURCE_MCP, "mcp");
        assert_eq!(SERVER_KEY_GITHUB, "github");
    }

    #[test]
    fn save_semantics() {
        assert_eq!(tool_sequence_id(None), 1);
        assert_eq!(tool_sequence_id(Some(5)), 6);
        assert_eq!(saved_sequence_id(None), 1);
        assert_eq!(saved_sequence_id(Some(0)), 1);
        assert_eq!(saved_sequence_id(Some(5)), 6);
        assert_eq!(saved_sort_order(None), 65535.0);
        assert_eq!(saved_sort_order(Some(100.0)), 10100.0);
        assert_eq!(ISSUE_SORT_ORDER_DEFAULT, 65535.0);
        assert_eq!(ISSUE_SORT_ORDER_STEP, 10000.0);
        // CPython oracle for convert_uuid_to_integer.
        assert_eq!(
            advisory_lock_key(&Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap()),
            8400349069047396436
        );
    }

    /// `(columns, values)` of a flat `INSERT INTO t (...) VALUES (...)`.
    fn insert_arity(sql: &str) -> (usize, usize) {
        let columns = sql.split("VALUES").next().expect("columns");
        let values = sql.split("VALUES").nth(1).expect("values");
        (
            columns.matches(',').count() + 1,
            values.matches(',').count() + 1,
        )
    }

    #[test]
    fn sql_text() {
        for sql in [
            LEDGER_INSERT_WRITE_SQL,
            LEDGER_INSERT_READ_SQL,
            COMMENT_INSERT_SQL,
            COMMENT_DESCRIPTION_INSERT_SQL,
            CREATE_ISSUE_INSERT_SQL,
            CREATE_ISSUE_SEQUENCE_INSERT_SQL,
        ] {
            assert_eq!(insert_arity(sql).0, insert_arity(sql).1, "{sql}");
        }
        for fragment in [
            "FROM agent_run",
            "INNER JOIN users ON users.id = agent_run.created_by_id",
            "INNER JOIN workspaces ON workspaces.id = agent_run.workspace_id",
            "LEFT OUTER JOIN issues AS work_item",
            "LEFT OUTER JOIN scheduler_bindings AS scheduler_binding",
            "INNER JOIN pod ON pod.id = agent_run.pod_id",
            "users.is_active, users.is_bot, workspaces.slug",
            "WHERE agent_run.id = $1 LIMIT 1",
        ] {
            assert!(SCOPE_RUN_SQL.contains(fragment), "{fragment}");
        }
        assert!(SCOPE_WORKSPACE_MEMBER_SQL.contains("workspace_id = $2"));
        assert!(SCOPE_WORKSPACE_MEMBER_SQL.contains("member_id = $1"));
        assert!(PROJECT_READ_GATE_SQL.contains("role IN (20, 15, 5)"));
        assert!(PROJECT_READ_GATE_SQL.contains("workspaces.slug = $2"));
        assert!(PROJECT_READ_GATE_SQL.contains("role = 20"));
        // The write bit reuses the assistant gate verbatim: pin its shape.
        assert!(crate::assistant::tools_issues::PROJECT_WRITE_GATE_SQL.contains("role IN (20, 15)"));
        // ... and the relation pool reuses its scoping: pin the $1/$2 bindings.
        assert!(crate::assistant::tools_issues::SCOPED_ISSUES_SQL
            .contains("project_members.member_id = $1"));
        assert!(crate::assistant::tools_issues::SCOPED_ISSUES_SQL.contains("workspaces.slug = $2"));
        assert_eq!(OWN_PROJECT_ISSUES_PREDICATE, "issues.project_id = $3");
        for fragment in [
            "agent_run_id = $1",
            "tool_call_id = $2",
            "ORDER BY id ASC LIMIT 1",
        ] {
            assert!(FIND_EXISTING_CALL_SQL.contains(fragment), "{fragment}");
        }
        assert!(COUNT_SUCCEEDED_WRITES_SQL.contains("risk = 'write' AND status = 'succeeded'"));
        assert!(WRITE_TOOL_USED_SQL.contains("tool_name = $2"));
        assert!(LEDGER_INSERT_WRITE_SQL.contains("'write', 'prepared'"));
        assert!(LEDGER_INSERT_READ_SQL.contains("'read', 'submitted'"));
        assert!(LEDGER_MARK_SUBMITTED_SQL.contains("status = 'submitted'"));
        assert!(LEDGER_MARK_SUCCEEDED_SQL.contains("safe_replay_result = $2"));
        assert!(LEDGER_MARK_FAILED_SQL.contains("error_code = $2"));
        assert!(CURRENT_ISSUE_COMMENTS_SQL.contains("ORDER BY created_at DESC LIMIT 50"));
        assert!(PROJECT_STATES_SQL.contains("ORDER BY sequence ASC"));
        assert!(PROJECT_STATES_SQL.contains("NOT (states.group = 'triage')"));
        assert!(SEARCH_ISSUES_SQL.contains("SELECT DISTINCT"));
        assert!(SEARCH_ISSUES_SQL.contains("UPPER(issues.name::text) LIKE UPPER('%' || $2 || '%')"));
        assert!(SEARCH_ISSUES_SQL
            .contains("UPPER(issues.description_stripped::text) LIKE UPPER('%' || $2 || '%')"));
        assert!(!SEARCH_ISSUES_SQL.contains("ILIKE"));
        assert!(SEARCH_ISSUES_SQL.contains("LIMIT $3"));
        assert!(!SEARCH_ISSUES_SQL.contains("ORDER BY"));
        assert!(GET_PROJECT_ISSUE_SQL.contains("issues.id = $1 AND issues.project_id = $2"));
        assert!(LINKED_REVIEWS_SQL.contains("ORDER BY created_at DESC LIMIT 20"));
        assert!(COMMENT_INSERT_SQL.contains("'Pi Dash Cloud Agent'"));
        assert!(COMMENT_INSERT_SQL.contains("'INTERNAL'"));
        assert!(COMMENT_INSERT_SQL.contains("attachments"));
        assert!(COMMENT_DESCRIPTION_INSERT_SQL.contains("INSERT INTO descriptions"));
        assert!(COMMENT_DESCRIPTION_INSERT_SQL.contains("description_binary"));
        assert!(COMMENT_DESCRIPTION_LINK_SQL.contains("description_id = $2"));
        assert!(WORKPAD_UPDATE_SQL.contains("SET workpad = $3"));
        assert!(TRANSITION_STATE_GET_SQL.contains("FROM states"));
        assert!(TRANSITION_ISSUE_LOCK_SQL.contains("FOR UPDATE OF issues"));
        assert!(!TRANSITION_ISSUE_LOCK_SQL.contains("LIMIT"));
        assert!(TRANSITION_ISSUE_SAVE_SQL.contains("updated_by_id = $4"));
        assert!(CREATE_PROJECT_LOCK_SQL.contains("FOR UPDATE"));
        assert!(CREATE_DEFAULT_STATE_SQL.contains("\"default\""));
        assert!(CREATE_BACKLOG_STATE_SQL.contains("states.group = 'backlog'"));
        assert!(CREATE_SEQ_MAX_SQL.contains("MAX(sequence_id)"));
        assert!(CREATE_SAVE_DEFAULT_STATE_SQL.contains("NOT is_triage"));
        assert_eq!(CREATE_ADVISORY_LOCK_SQL, "SELECT pg_advisory_xact_lock($1)");
        assert!(CREATE_SEQ_SCAN_SQL.contains("FROM issue_sequences"));
        assert!(!CREATE_SEQ_SCAN_SQL.contains("deleted ="));
        assert!(CREATE_SORT_ORDER_MAX_SQL.contains("state_id = $2"));
        assert!(CREATE_ISSUE_INSERT_SQL.contains("'cloud_agent'"));
        assert!(CREATE_ISSUE_INSERT_SQL.contains("'none', 0"));
        assert!(CREATE_ISSUE_INSERT_SQL.contains("agent_executor"));
        assert!(CREATE_ISSUE_SEQUENCE_INSERT_SQL.contains("INSERT INTO issue_sequences"));
        assert!(CREATE_ISSUE_SEQUENCE_INSERT_SQL.contains(", FALSE)"));
        assert!(GITHUB_BINDING_SQL.contains("git_repository_bindings AS b"));
        assert!(GITHUB_INSTALLATION_SQL.contains("ORDER BY created_at DESC LIMIT 1"));
        assert!(LINKED_PR_SQL.contains("namespace = $2 AND repo_name = $3"));
    }
}
