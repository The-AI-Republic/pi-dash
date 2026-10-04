#![forbid(unsafe_code)]

//! App issue-detail read shapes for app:issues (D-26).
//!
//! Port of `apps/api/pi_dash/app/serializers/issue.py:1039-1113` (the
//! 29-key `IssueSerializer` read shape), `:1219-1225`
//! (`IssueLiteSerializer`), `:1226-1414` (`IssueDetailSerializer`) and
//! `:1415-1440` (`IssuePublicSerializer`). Consumers: the retrieve /
//! identifier / archive-retrieve handlers (PIDASHCONV-651/652/656).
//!
//! Reuse (merged, called not copied):
//!
//! * `issue_is_actively_synced` (`super::serializers_engage`, PIDASHCONV-642)
//!   computes the `is_synced` value the caller puts in
//!   [`IssueDetailBaseRow::is_synced`]; the sync probes stay caller-side so
//!   `to_representation` stays pure, like every sibling module.
//! * The ticker math (`effective_max_ticks` / `remaining` / `cap_reached`)
//!   runs on the merged D-10 [`IssueAgentTicker`](pidash_db::tasks_ticker::IssueAgentTicker)
//!   struct; the pool and the stage interval resolve through the merged D-12
//!   [`pool_size`](crate::orchestration::clock) /
//!   [`effective_interval_seconds`](crate::orchestration::clock) over the
//!   caller's [`ProjectClockPolicy`](crate::orchestration::clock).
//! * `can_re_tick` calls the merged D-12
//!   [`is_ticking_state`](pidash_types::orchestration) (PIDASHCONV-545) and
//!   [`is_paused_state`](crate::orchestration::clock) (PIDASHCONV-580).
//! * The blocker fields select out of the merged D-12
//!   [`RelationsSummary`](crate::orchestration::blockers) (PIDASHCONV-561).
//!   The Python per-instance `_blocker_summary_cache` (`:1250-1258`) is a
//!   one-call-per-issue memo; the Rust caller runs the merged
//!   `relations_summary` once and passes the value to both selectors, which
//!   is the same single call.
//! * `error_diagnostic` calls the merged D-15
//!   [`classify_run_error`](pidash_types::runner_runs) (same
//!   `runner/diagnostics.py:173-244` unit, FX-pinned in types) — not a new
//!   port. D-32's `api/src/app_intake/issues.rs` copy is api-private and,
//!   judged against the Python source, diverges twice (live-state usage
//!   keys, the observed-`None` guard), so it is not followed here.
//! * The live-state token properties run through the merged
//!   [`flat_token_fields`](pidash_types::runner_runs) over the `usage` JSON.
//! * The public nests call the merged app twins
//!   [`state_lite_to_representation`](super::serializers_refs),
//!   [`project_lite_to_representation`](super::serializers_refs) and
//!   [`user_lite_to_representation`](super::serializers_links).
//!
//! Deliberately NOT reused: space `issue_to_representation`
//! (`space::serializers::issue_graph`) emits the nested space shape (14
//! nests + all columns), not this flat 29-key app shape — the two share no
//! logic, only a model, so the base lives here (same call pattern as
//! PIDASHCONV-642's sync guards, which the fixture blesses as "not a dup").
//! The app reaction (`__all__` + `actor_detail`) and vote (6 keys +
//! `actor_detail`) twins likewise differ from the space 5-key shapes and
//! are ported here; the space twin is wire-incompatible by construction.
//!
//! Datetimes render in two shapes, exactly as the Python does: the base /
//! reaction / vote auto fields use DRF's format (`Z` suffix — `USE_TZ` +
//! `TIME_ZONE = "UTC"`, `settings/common.py:361-362`), while the detail
//! serializer's own `_serialize_datetime` (`:1247-1248`) and the ticker
//! datetimes (`:1309-1310`) use plain `datetime.isoformat()` (`+00:00`,
//! microsecond digits only when nonzero). [`serialize_drf_datetime`] and
//! [`serialize_iso_datetime`] pin both; rows carry the rendered strings,
//! following the sibling convention.
//!
//! Out of scope here: `IssueSerializer.validate` (`:1099-1113`) is a write
//! guard, and the D-26 routes use the base/detail serializers read-only —
//! every write action routes through `IssueCreateSerializer`
//! (`base.py:207-208`), which extends `BaseSerializer`, not
//! `IssueSerializer`, so the guard never runs on these paths.
//!
//! Fixture: `rust-api/fixtures/app_issues/serializers/FX-ISS-02.detail.json`.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * Retrieve / archive-retrieve omit the `is_intake` key (unannotated
//!   attribute → DRF `SkipField`) while identifier responses include it —
//!   one serializer, two key sets. Reproduced via `Option<bool>` +
//!   `skip_serializing_if` ([`IssueDetailRow::is_intake`]).

use chrono::{DateTime, Utc};
use serde::Serialize;

use pidash_db::tasks_ticker::IssueAgentTicker;
use pidash_types::orchestration::{is_ticking_state, StateRef};
use pidash_types::runner_runs::{classify_run_error, flat_token_fields, RunErrorDiagnostic};

use super::serializers_links::{user_lite_to_representation, UserLiteRow, UserLiteView};
use super::serializers_refs::{
    project_lite_to_representation, state_lite_to_representation, ProjectLiteRow, ProjectLiteView,
    StateLiteRow, StateLiteView,
};
use crate::orchestration::blockers::{RelationDirections, RelationsSummary};
use crate::orchestration::clock::{
    effective_interval_seconds, is_paused_state, pool_size, ProjectClockPolicy,
};

/// Python `datetime.isoformat()` for an aware UTC timestamp (`issue.py:1247-1248`,
/// `:1309-1310`): `+00:00` offset, six microsecond digits only when nonzero.
/// Same rule as the `prompting` port (chrono `AutoSi` would trim `.123000`
/// to `.123`; Python always prints six digits).
pub fn serialize_iso_datetime(value: DateTime<Utc>) -> String {
    if value.timestamp_subsec_micros() == 0 {
        value.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
    } else {
        value.to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
    }
}

/// DRF `DateTimeField` rendering for an aware UTC timestamp (the auto fields
/// of the base / reaction / vote shapes): `isoformat` with the `+00:00`
/// replaced by `Z` (DRF `ISO_8601` branch; `TIME_ZONE = "UTC"` makes the
/// `enforce_timezone` conversion a no-op).
pub fn serialize_drf_datetime(value: DateTime<Utc>) -> String {
    if value.timestamp_subsec_micros() == 0 {
        value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    } else {
        value.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
    }
}

/// App `IssueSerializer` wire keys (`issue.py:1063-1093`), in
/// `Meta.fields` order: the 29-key base every detail response starts with.
/// Count the list: 29.
pub const ISSUE_DETAIL_BASE_FIELDS: [&str; 29] = [
    "id",
    "name",
    "state_id",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "complexity_score",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "cycle_id",
    "assigned_pod_id",
    "agent_executor",
    "module_ids",
    "label_ids",
    "assignee_ids",
    "sub_issues_count",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "attachment_count",
    "link_count",
    "is_draft",
    "archived_at",
    "is_synced",
];

/// An `Issue` row for the 29-key base shape (`db/models/issue.py:115-229`):
/// `state_id` / `estimate_point` / `parent_id` / `cycle_id` (annotation) /
/// `assigned_pod_id` / `created_by` / `updated_by` render FK ids or `None`;
/// the `*_id` attnames are DRF property-fallback `ReadOnlyField`s (same rule
/// as the label port); `module_ids` / `label_ids` / `assignee_ids` and the
/// three counts ride queryset annotations; datetimes are pre-rendered DRF
/// strings; `archived_at` is a `DateField`; `is_synced` is the merged
/// `issue_is_actively_synced` verdict, computed caller-side.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueDetailBaseRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub state_id: Option<&'a str>,
    pub sort_order: f64,
    pub completed_at: Option<&'a str>,
    pub estimate_point: Option<&'a str>,
    pub priority: &'a str,
    pub complexity_score: i32,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i32,
    pub project_id: &'a str,
    pub parent_id: Option<&'a str>,
    pub cycle_id: Option<&'a str>,
    pub assigned_pod_id: Option<&'a str>,
    pub agent_executor: Option<&'a str>,
    pub module_ids: Vec<&'a str>,
    pub label_ids: Vec<&'a str>,
    pub assignee_ids: Vec<&'a str>,
    pub sub_issues_count: i64,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub attachment_count: i64,
    pub link_count: i64,
    pub is_draft: bool,
    pub archived_at: Option<&'a str>,
    pub is_synced: bool,
}

/// App `IssueSerializer.to_representation` output (`issue.py:1039-1113`), in
/// `Meta.fields` order. `sort_order` goes through `serde` `f64` (same caveat
/// as the merged space port — exact for realistic orders).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueDetailBaseView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub state_id: Option<&'a str>,
    pub sort_order: f64,
    pub completed_at: Option<&'a str>,
    pub estimate_point: Option<&'a str>,
    pub priority: &'a str,
    pub complexity_score: i32,
    pub start_date: Option<&'a str>,
    pub target_date: Option<&'a str>,
    pub sequence_id: i32,
    pub project_id: &'a str,
    pub parent_id: Option<&'a str>,
    pub cycle_id: Option<&'a str>,
    pub assigned_pod_id: Option<&'a str>,
    pub agent_executor: Option<&'a str>,
    pub module_ids: Vec<&'a str>,
    pub label_ids: Vec<&'a str>,
    pub assignee_ids: Vec<&'a str>,
    pub sub_issues_count: i64,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub attachment_count: i64,
    pub link_count: i64,
    pub is_draft: bool,
    pub archived_at: Option<&'a str>,
    pub is_synced: bool,
}

/// Port of the app `IssueSerializer` read shape (`issue.py:1039-1113`).
/// Field-for-field copy.
pub fn issue_detail_base_to_representation<'a>(
    row: &'a IssueDetailBaseRow<'a>,
) -> IssueDetailBaseView<'a> {
    IssueDetailBaseView {
        id: row.id,
        name: row.name,
        state_id: row.state_id,
        sort_order: row.sort_order,
        completed_at: row.completed_at,
        estimate_point: row.estimate_point,
        priority: row.priority,
        complexity_score: row.complexity_score,
        start_date: row.start_date,
        target_date: row.target_date,
        sequence_id: row.sequence_id,
        project_id: row.project_id,
        parent_id: row.parent_id,
        cycle_id: row.cycle_id,
        assigned_pod_id: row.assigned_pod_id,
        agent_executor: row.agent_executor,
        module_ids: row.module_ids.clone(),
        label_ids: row.label_ids.clone(),
        assignee_ids: row.assignee_ids.clone(),
        sub_issues_count: row.sub_issues_count,
        created_at: row.created_at,
        updated_at: row.updated_at,
        created_by: row.created_by,
        updated_by: row.updated_by,
        attachment_count: row.attachment_count,
        link_count: row.link_count,
        is_draft: row.is_draft,
        archived_at: row.archived_at,
        is_synced: row.is_synced,
    }
}

/// `get_agent_ticker` wire keys (`issue.py:1298-1317`), in dict order: 14.
pub const AGENT_TICKER_FIELDS: [&str; 14] = [
    "enabled",
    "user_disabled",
    "used",
    "tick_count",
    "granted",
    "waited",
    "max_ticks",
    "remaining",
    "interval_seconds",
    "next_run_at",
    "last_tick_at",
    "disarm_reason",
    "pending_entry",
    "can_re_tick",
];

/// `get_agent_ticker` output (`issue.py:1268-1318`), in dict order.
/// `remaining` is `None` (rendered `null`) on an infinite pool;
/// `max_ticks` is `-1` there. The datetimes render plain `isoformat`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentTickerView<'a> {
    pub enabled: bool,
    pub user_disabled: bool,
    pub used: i32,
    pub tick_count: i32,
    pub granted: i32,
    pub waited: i32,
    pub max_ticks: i32,
    pub remaining: Option<i32>,
    pub interval_seconds: i64,
    pub next_run_at: Option<String>,
    pub last_tick_at: Option<String>,
    pub disarm_reason: &'a str,
    pub pending_entry: bool,
    pub can_re_tick: bool,
}

/// Port of `get_agent_ticker` (`issue.py:1268-1318`).
///
/// `ticker` is the issue's `agent_ticker` row (`None` at the call boundary
/// renders `None`, `:1275-1281`); `policy` carries the project pool +
/// cadence columns; `state` is the issue's current state for
/// `can_re_tick` (`(is_ticking_state or is_paused_state) and cap_reached`,
/// `:1296`) and the stage interval. Pool math, predicates and interval all
/// run through the merged D-10/D-12 ports.
pub fn agent_ticker_to_representation<'a>(
    ticker: &'a IssueAgentTicker,
    policy: &ProjectClockPolicy,
    state: Option<&StateRef<'_>>,
) -> AgentTickerView<'a> {
    let pool = pool_size(policy);
    let can_re_tick =
        (is_ticking_state(state) || is_paused_state(state)) && ticker.cap_reached(pool);
    AgentTickerView {
        enabled: ticker.enabled,
        user_disabled: ticker.user_disabled,
        used: ticker.used,
        tick_count: ticker.used,
        granted: ticker.granted,
        waited: ticker.waited,
        max_ticks: ticker.effective_max_ticks(pool),
        remaining: ticker.remaining(pool),
        interval_seconds: effective_interval_seconds(state, policy),
        next_run_at: ticker.next_run_at.map(serialize_iso_datetime),
        last_tick_at: ticker.last_tick_at.map(serialize_iso_datetime),
        disarm_reason: ticker.disarm_reason.as_str(),
        pending_entry: ticker.pending_entry,
        can_re_tick,
    }
}

/// `_serialize_agent_live_state` wire keys (`issue.py:1320-1337`), in dict
/// order: 13.
pub const AGENT_LIVE_STATE_FIELDS: [&str; 13] = [
    "observed_run_id",
    "last_event_at",
    "last_event_kind",
    "last_event_summary",
    "agent_pid",
    "agent_subprocess_alive",
    "approvals_pending",
    "input_tokens",
    "output_tokens",
    "total_tokens",
    "llm_model",
    "turn_count",
    "updated_at",
];

/// A `RunnerLiveState` row for agent rendering
/// (`runner/models.py:1454-1516`): every column nullable except `updated_at`
/// (`auto_now`). The token counts are properties over `usage`
/// (`:1510-1516`), not columns — the port reads them through the merged
/// `flat_token_fields`, so `usage` is carried as JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentLiveStateRow<'a> {
    pub observed_run_id: Option<&'a str>,
    pub last_event_at: Option<&'a str>,
    pub last_event_kind: Option<&'a str>,
    pub last_event_summary: Option<&'a str>,
    pub agent_pid: Option<i32>,
    pub agent_subprocess_alive: Option<bool>,
    pub approvals_pending: Option<i32>,
    pub usage: &'a serde_json::Value,
    pub llm_model: Option<&'a str>,
    pub turn_count: Option<i32>,
    pub updated_at: &'a str,
}

/// `_serialize_agent_live_state` output (`issue.py:1320-1337`), in dict
/// order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentLiveStateView<'a> {
    pub observed_run_id: Option<&'a str>,
    pub last_event_at: Option<&'a str>,
    pub last_event_kind: Option<&'a str>,
    pub last_event_summary: Option<&'a str>,
    pub agent_pid: Option<i32>,
    pub agent_subprocess_alive: Option<bool>,
    pub approvals_pending: Option<i32>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub llm_model: Option<&'a str>,
    pub turn_count: Option<i32>,
    pub updated_at: &'a str,
}

/// Port of `_serialize_agent_live_state` (`issue.py:1320-1337`)
/// (`None` at the call boundary renders `None`, `:1321-1322`).
pub fn agent_live_state_to_representation<'a>(
    row: &'a AgentLiveStateRow<'a>,
) -> AgentLiveStateView<'a> {
    let tokens = flat_token_fields(row.usage);
    AgentLiveStateView {
        observed_run_id: row.observed_run_id,
        last_event_at: row.last_event_at,
        last_event_kind: row.last_event_kind,
        last_event_summary: row.last_event_summary,
        agent_pid: row.agent_pid,
        agent_subprocess_alive: row.agent_subprocess_alive,
        approvals_pending: row.approvals_pending,
        input_tokens: tokens.input_tokens,
        output_tokens: tokens.output_tokens,
        total_tokens: tokens.total_tokens,
        llm_model: row.llm_model,
        turn_count: row.turn_count,
        updated_at: row.updated_at,
    }
}

/// `_serialize_agent_run` wire keys (`issue.py:1352-1371`), in dict order:
/// 19.
pub const AGENT_RUN_FIELDS: [&str; 19] = [
    "id",
    "status",
    "executor_kind",
    "queue_position",
    "runner",
    "runner_name",
    "created_at",
    "assigned_at",
    "started_at",
    "ended_at",
    "done_payload",
    "error",
    "error_code",
    "error_diagnostic",
    "llm_model",
    "input_tokens",
    "output_tokens",
    "total_tokens",
    "live_state",
];

/// One agent-run row with its runner facts for the status render: the
/// `agent_run` columns the serializer reads (`runner/models.py:872-1036`) —
/// `id` / `status` / `executor_kind` (non-null, defaulted),
/// `queue_position` (nullable smallint), `runner_id` + the joined
/// `runner.name`, `created_at` (non-null) + the three nullable stamps,
/// `done_payload` (nullable JSON), `error` / `error_code` / `llm_model`
/// (non-null, default `""`), the three generated nullable token bigints —
/// plus the joined live-state row. Datetimes are pre-rendered `isoformat`
/// strings.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentRunDetailRow<'a> {
    pub id: &'a str,
    pub status: &'a str,
    pub executor_kind: &'a str,
    pub queue_position: Option<i16>,
    pub runner_id: Option<&'a str>,
    pub runner_name: Option<&'a str>,
    pub created_at: &'a str,
    pub assigned_at: Option<&'a str>,
    pub started_at: Option<&'a str>,
    pub ended_at: Option<&'a str>,
    pub done_payload: Option<&'a serde_json::Value>,
    pub error: &'a str,
    pub error_code: &'a str,
    pub llm_model: &'a str,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub live_state: Option<AgentLiveStateRow<'a>>,
}

/// `_serialize_agent_run` output (`issue.py:1339-1371`), in dict order.
/// `error_diagnostic` is the merged `classify_run_error` verdict.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentRunView<'a> {
    pub id: &'a str,
    pub status: &'a str,
    pub executor_kind: &'a str,
    pub queue_position: Option<i16>,
    pub runner: Option<&'a str>,
    pub runner_name: Option<&'a str>,
    pub created_at: &'a str,
    pub assigned_at: Option<&'a str>,
    pub started_at: Option<&'a str>,
    pub ended_at: Option<&'a str>,
    pub done_payload: Option<&'a serde_json::Value>,
    pub error: &'a str,
    pub error_code: &'a str,
    pub error_diagnostic: Option<RunErrorDiagnostic>,
    pub llm_model: &'a str,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub live_state: Option<AgentLiveStateView<'a>>,
}

/// Port of `_serialize_agent_run` (`issue.py:1339-1371`).
///
/// `None` renders `None` (`:1340-1341`). The live-state guard is verbatim:
/// the joined row is used only with `include_live_state`, a non-null
/// `runner_id`, and an `observed_run_id` that is `None` or equals the run
/// id (`:1343-1350`) — a `None` observed id keeps the block.
pub fn agent_run_to_representation<'a>(
    run: Option<&'a AgentRunDetailRow<'a>>,
    include_live_state: bool,
) -> Option<AgentRunView<'a>> {
    let run = run?;
    let mut live_state = None;
    if include_live_state && run.runner_id.is_some() {
        live_state = run.live_state.as_ref().and_then(|state| {
            let stale = state
                .observed_run_id
                .is_some_and(|observed| observed != run.id);
            if stale {
                None
            } else {
                Some(agent_live_state_to_representation(state))
            }
        });
    }
    Some(AgentRunView {
        id: run.id,
        status: run.status,
        executor_kind: run.executor_kind,
        queue_position: run.queue_position,
        runner: run.runner_id,
        runner_name: run.runner_id.and(run.runner_name),
        created_at: run.created_at,
        assigned_at: run.assigned_at,
        started_at: run.started_at,
        ended_at: run.ended_at,
        done_payload: run.done_payload,
        error: run.error,
        error_code: run.error_code,
        error_diagnostic: classify_run_error(Some(run.error)),
        llm_model: run.llm_model,
        input_tokens: run.input_tokens,
        output_tokens: run.output_tokens,
        total_tokens: run.total_tokens,
        live_state,
    })
}

/// `get_agent_status` wire keys (`issue.py:1407-1412`), in dict order: 4.
pub const AGENT_STATUS_FIELDS: [&str; 4] = ["ticker", "active_run", "latest_run", "run_count"];

/// `get_agent_status` output (`issue.py:1373-1412`), in dict order. The
/// `ticker` value duplicates the top-level `agent_ticker` block (Python
/// calls `get_agent_ticker` once per block, `:1383` + `:1408`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentStatusView<'a> {
    pub ticker: Option<AgentTickerView<'a>>,
    pub active_run: Option<AgentRunView<'a>>,
    pub latest_run: Option<AgentRunView<'a>>,
    pub run_count: i64,
}

/// Port of `get_agent_status` (`issue.py:1373-1412`).
///
/// `ticker` is the already-rendered `agent_ticker` block (or `None` without
/// a ticker row); `active_run` / `latest_run` are the
/// [`ACTIVE_AGENT_RUN_SQL`] / [`LATEST_AGENT_RUN_SQL`] rows (or `None`
/// without one); `run_count` is the [`AGENT_RUN_COUNT_SQL`] verdict.
/// Returns `None` when the ticker block is `None` and there is no latest
/// run (`:1404-1405`). The active run always carries live state; the latest
/// carries it only when there is no active run (`:1410-1411`).
pub fn agent_status_to_representation<'a>(
    ticker: Option<AgentTickerView<'a>>,
    active_run: Option<&'a AgentRunDetailRow<'a>>,
    latest_run: Option<&'a AgentRunDetailRow<'a>>,
    run_count: i64,
) -> Option<AgentStatusView<'a>> {
    if ticker.is_none() && latest_run.is_none() {
        return None;
    }
    Some(AgentStatusView {
        ticker,
        active_run: agent_run_to_representation(active_run, true),
        latest_run: agent_run_to_representation(latest_run, active_run.is_none()),
        run_count,
    })
}

/// `get_relations_summary` (`issue.py:1260-1263`): the `relations_summary`
/// half of one cached [`RelationsSummary`].
pub fn relations_summary_field(summary: &RelationsSummary) -> &RelationDirections {
    &summary.relations_summary
}

/// `get_has_open_blockers` (`issue.py:1265-1266`): the `has_open_blockers`
/// half of one cached [`RelationsSummary`].
pub fn has_open_blockers_field(summary: &RelationsSummary) -> bool {
    summary.has_open_blockers
}

/// `IssueDetailSerializer` wire keys (`issue.py:1237-1245`), in
/// `Meta.fields` order: the 29 base keys plus the 7 added keys. 36.
pub const ISSUE_DETAIL_FIELDS: [&str; 36] = [
    "id",
    "name",
    "state_id",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "complexity_score",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "cycle_id",
    "assigned_pod_id",
    "agent_executor",
    "module_ids",
    "label_ids",
    "assignee_ids",
    "sub_issues_count",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "attachment_count",
    "link_count",
    "is_draft",
    "archived_at",
    "is_synced",
    "description_html",
    "is_subscribed",
    "is_intake",
    "agent_ticker",
    "agent_status",
    "relations_summary",
    "has_open_blockers",
];

/// The retrieve / archive-retrieve key set: [`ISSUE_DETAIL_FIELDS`] minus
/// `is_intake`, which those views do not annotate, so DRF's `SkipField`
/// omits the key (`base.py:486-618`, `archive.py:222-255`). 35.
pub const ISSUE_DETAIL_RETRIEVE_FIELDS: [&str; 35] = [
    "id",
    "name",
    "state_id",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "complexity_score",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "cycle_id",
    "assigned_pod_id",
    "agent_executor",
    "module_ids",
    "label_ids",
    "assignee_ids",
    "sub_issues_count",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "attachment_count",
    "link_count",
    "is_draft",
    "archived_at",
    "is_synced",
    "description_html",
    "is_subscribed",
    "agent_ticker",
    "agent_status",
    "relations_summary",
    "has_open_blockers",
];

/// The ticker half of an issue-detail row: the `agent_ticker` row, the
/// project clock policy, and the issue's current state (for `can_re_tick`
/// and the stage interval).
#[derive(Debug, Clone, PartialEq)]
pub struct AgentTickerInput<'a> {
    pub ticker: &'a IssueAgentTicker,
    pub policy: &'a ProjectClockPolicy,
    pub state: Option<StateRef<'a>>,
}

/// An issue-detail row: the 29-key base, the two existence annotations
/// (`is_intake` is `None` when the queryset lacks the annotation —
/// retrieve / archive-retrieve — reproducing the `SkipField` omission),
/// the ticker input (or `None` without a ticker row), the latest + active
/// run rows (or `None` without one), the total run count, and the one
/// cached blocker summary both blocker fields select out of.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueDetailRow<'a> {
    pub base: IssueDetailBaseRow<'a>,
    pub description_html: &'a str,
    pub is_subscribed: bool,
    pub is_intake: Option<bool>,
    pub ticker: Option<AgentTickerInput<'a>>,
    pub latest_run: Option<AgentRunDetailRow<'a>>,
    pub active_run: Option<AgentRunDetailRow<'a>>,
    pub run_count: i64,
    pub blockers: &'a RelationsSummary,
}

/// `IssueDetailSerializer.to_representation` output (`issue.py:1226-1414`),
/// in `Meta.fields` order. The flattened base keeps its 29 keys first;
/// `is_intake` is skipped when the row lacks the annotation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueDetailView<'a> {
    #[serde(flatten)]
    pub base: IssueDetailBaseView<'a>,
    pub description_html: &'a str,
    pub is_subscribed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_intake: Option<bool>,
    pub agent_ticker: Option<AgentTickerView<'a>>,
    pub agent_status: Option<AgentStatusView<'a>>,
    pub relations_summary: &'a RelationDirections,
    pub has_open_blockers: bool,
}

/// Port of `IssueDetailSerializer` (`issue.py:1226-1414`). The ticker block
/// renders once and is cloned into the status envelope (same value Python
/// computes per block).
pub fn issue_detail_to_representation<'a>(row: &'a IssueDetailRow<'a>) -> IssueDetailView<'a> {
    let agent_ticker = row.ticker.as_ref().map(|input| {
        agent_ticker_to_representation(input.ticker, input.policy, input.state.as_ref())
    });
    let agent_status = agent_status_to_representation(
        agent_ticker.clone(),
        row.active_run.as_ref(),
        row.latest_run.as_ref(),
        row.run_count,
    );
    IssueDetailView {
        base: issue_detail_base_to_representation(&row.base),
        description_html: row.description_html,
        is_subscribed: row.is_subscribed,
        is_intake: row.is_intake,
        agent_ticker,
        agent_status,
        relations_summary: relations_summary_field(row.blockers),
        has_open_blockers: has_open_blockers_field(row.blockers),
    }
}

/// App `IssueLiteSerializer` wire keys (`issue.py:1221-1223`), in
/// `Meta.fields` order.
pub const ISSUE_LITE_FIELDS: [&str; 3] = ["id", "sequence_id", "project_id"];

/// An `Issue` row for lite rendering: the pk, the sequence number, and the
/// non-null project FK id.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueLiteRow<'a> {
    pub id: &'a str,
    pub sequence_id: i32,
    pub project_id: &'a str,
}

/// App `IssueLiteSerializer.to_representation` output (`issue.py:1219-1225`),
/// in `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueLiteView<'a> {
    pub id: &'a str,
    pub sequence_id: i32,
    pub project_id: &'a str,
}

/// Port of the app `IssueLiteSerializer` (`issue.py:1219-1225`).
/// Field-for-field copy.
pub fn issue_lite_to_representation<'a>(row: &'a IssueLiteRow<'a>) -> IssueLiteView<'a> {
    IssueLiteView {
        id: row.id,
        sequence_id: row.sequence_id,
        project_id: row.project_id,
    }
}

/// App `IssueReactionSerializer` wire keys (`issue.py:900-906`):
/// `fields = "__all__"` plus the declared `actor_detail` nest, in DRF
/// default order — `[pk] + declared + non-relational columns + FK columns`
/// (probed with a lookalike hierarchy on DRF: `id` carries
/// `serialize=False`, so it appears once). 12.
pub const APP_ISSUE_REACTION_FIELDS: [&str; 12] = [
    "id",
    "actor_detail",
    "created_at",
    "updated_at",
    "deleted_at",
    "reaction",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "actor",
    "issue",
];

/// An `IssueReaction` row for app rendering (`db/models/issue.py:726-733`):
/// the pk, the actor lite nest, DRF datetimes, the reaction text, and the
/// six FK ids (`created_by` / `updated_by` nullable).
#[derive(Debug, Clone, PartialEq)]
pub struct AppIssueReactionRow<'a> {
    pub id: &'a str,
    pub actor_detail: UserLiteRow<'a>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub reaction: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub actor: &'a str,
    pub issue: &'a str,
}

/// App `IssueReactionSerializer.to_representation` output
/// (`issue.py:900-906`), in wire order. This is the app twin — the space
/// 5-key shape (`issue_graph.rs`) is wire-incompatible by construction.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppIssueReactionView<'a> {
    pub id: &'a str,
    pub actor_detail: UserLiteView<'a>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub reaction: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub project: &'a str,
    pub workspace: &'a str,
    pub actor: &'a str,
    pub issue: &'a str,
}

/// Port of the app `IssueReactionSerializer` (`issue.py:900-906`).
pub fn app_issue_reaction_to_representation<'a>(
    row: &'a AppIssueReactionRow<'a>,
) -> AppIssueReactionView<'a> {
    AppIssueReactionView {
        id: row.id,
        actor_detail: user_lite_to_representation(&row.actor_detail),
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        reaction: row.reaction,
        created_by: row.created_by,
        updated_by: row.updated_by,
        project: row.project,
        workspace: row.workspace,
        actor: row.actor,
        issue: row.issue,
    }
}

/// App `IssueVoteSerializer` wire keys (`issue.py:939-945`), in
/// `Meta.fields` order: 6.
pub const APP_ISSUE_VOTE_FIELDS: [&str; 6] = [
    "issue",
    "vote",
    "workspace",
    "project",
    "actor",
    "actor_detail",
];

/// An `IssueVote` row for app rendering (`db/models/issue.py:780-783`):
/// `vote` is `-1` (down) or `1` (up, the default).
#[derive(Debug, Clone, PartialEq)]
pub struct AppIssueVoteRow<'a> {
    pub issue: &'a str,
    pub vote: i32,
    pub workspace: &'a str,
    pub project: &'a str,
    pub actor: &'a str,
    pub actor_detail: UserLiteRow<'a>,
}

/// App `IssueVoteSerializer.to_representation` output (`issue.py:939-945`),
/// in `Meta.fields` order. The app twin — the space 5-key shape has no
/// `actor_detail`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppIssueVoteView<'a> {
    pub issue: &'a str,
    pub vote: i32,
    pub workspace: &'a str,
    pub project: &'a str,
    pub actor: &'a str,
    pub actor_detail: UserLiteView<'a>,
}

/// Port of the app `IssueVoteSerializer` (`issue.py:939-945`).
pub fn app_issue_vote_to_representation<'a>(row: &'a AppIssueVoteRow<'a>) -> AppIssueVoteView<'a> {
    AppIssueVoteView {
        issue: row.issue,
        vote: row.vote,
        workspace: row.workspace,
        project: row.project,
        actor: row.actor,
        actor_detail: user_lite_to_representation(&row.actor_detail),
    }
}

/// App `IssuePublicSerializer` wire keys (`issue.py:1424-1438`), in
/// `Meta.fields` order: 13.
pub const ISSUE_PUBLIC_FIELDS: [&str; 13] = [
    "id",
    "name",
    "description_html",
    "sequence_id",
    "state",
    "state_detail",
    "project",
    "project_detail",
    "workspace",
    "priority",
    "target_date",
    "reactions",
    "votes",
];

/// An `Issue` row for public rendering (`issue.py:1415-1440`): scalar
/// columns (`state` / `target_date` nullable), the state + project lite
/// nests, and the reaction (`source="issue_reactions"`) + vote nests in
/// `Meta.ordering` (`-created_at`) order, caller-side.
#[derive(Debug, Clone, PartialEq)]
pub struct IssuePublicRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description_html: &'a str,
    pub sequence_id: i32,
    pub state: Option<&'a str>,
    pub state_detail: StateLiteRow<'a>,
    pub project: &'a str,
    pub project_detail: ProjectLiteRow<'a>,
    pub workspace: &'a str,
    pub priority: &'a str,
    pub target_date: Option<&'a str>,
    pub reactions: Vec<AppIssueReactionRow<'a>>,
    pub votes: Vec<AppIssueVoteRow<'a>>,
}

/// App `IssuePublicSerializer.to_representation` output
/// (`issue.py:1415-1440`), in `Meta.fields` order. No view references this
/// shape and space defines its own twin — ported as-is.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssuePublicView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description_html: &'a str,
    pub sequence_id: i32,
    pub state: Option<&'a str>,
    pub state_detail: StateLiteView<'a>,
    pub project: &'a str,
    pub project_detail: ProjectLiteView<'a>,
    pub workspace: &'a str,
    pub priority: &'a str,
    pub target_date: Option<&'a str>,
    pub reactions: Vec<AppIssueReactionView<'a>>,
    pub votes: Vec<AppIssueVoteView<'a>>,
}

/// Port of the app `IssuePublicSerializer` (`issue.py:1415-1440`).
pub fn issue_public_to_representation<'a>(row: &'a IssuePublicRow<'a>) -> IssuePublicView<'a> {
    IssuePublicView {
        id: row.id,
        name: row.name,
        description_html: row.description_html,
        sequence_id: row.sequence_id,
        state: row.state,
        state_detail: state_lite_to_representation(&row.state_detail),
        project: row.project,
        project_detail: project_lite_to_representation(&row.project_detail),
        workspace: row.workspace,
        priority: row.priority,
        target_date: row.target_date,
        reactions: row
            .reactions
            .iter()
            .map(app_issue_reaction_to_representation)
            .collect(),
        votes: row
            .votes
            .iter()
            .map(app_issue_vote_to_representation)
            .collect(),
    }
}

/// `error_diagnostic` wire keys (`runner/diagnostics.py:173-244`), in Python
/// dict order: the merged [`RunErrorDiagnostic`] serializes these five.
pub const ERROR_DIAGNOSTIC_FIELDS: [&str; 5] =
    ["source", "source_label", "kind", "summary", "action"];

/// The eight `AgentRunStatus` values that gate the active run
/// (`issue.py:1386-1402`): `WAITING_FOR_WORKTREE` is retired but historical
/// rows still gate. Each literal equals the merged
/// `pidash_db::dispatch::AgentRunStatus` value (pinned by test).
pub const ACTIVE_RUN_STATUSES: [&str; 8] = [
    "queued",
    "assigned",
    "waiting_for_worktree",
    "running",
    "cancel_requested",
    "awaiting_approval",
    "awaiting_reauth",
    "paused_awaiting_input",
];

/// `runs.select_related("runner__live_state").order_by("-created_at").first()`
/// (`issue.py:1385`): newest run for the issue, whatever its status. `$1`
/// is the issue id. No tiebreak — Django emits none either. No
/// soft-delete guard — `AgentRun` is a plain `Model`. The projection is
/// the 16 `agent_run` columns the serializer reads, the joined
/// `runner.name`, and the 12 `runner_live_state` columns (Django would
/// select whole rows; this is the serializer-read subset, same rows).
pub const LATEST_AGENT_RUN_SQL: &str = "SELECT agent_run.id, agent_run.status, agent_run.executor_kind, agent_run.queue_position, agent_run.runner_id, agent_run.created_at, agent_run.assigned_at, agent_run.started_at, agent_run.ended_at, agent_run.done_payload, agent_run.error, agent_run.error_code, agent_run.llm_model, agent_run.input_tokens, agent_run.output_tokens, agent_run.total_tokens, runner.name, runner_live_state.observed_run_id, runner_live_state.last_event_at, runner_live_state.last_event_kind, runner_live_state.last_event_summary, runner_live_state.agent_pid, runner_live_state.agent_subprocess_alive, runner_live_state.approvals_pending, runner_live_state.usage, runner_live_state.llm_model, runner_live_state.turn_count, runner_live_state.updated_at FROM agent_run LEFT JOIN runner ON runner.id = agent_run.runner_id LEFT JOIN runner_live_state ON runner_live_state.runner_id = runner.id WHERE agent_run.work_item_id = $1 ORDER BY agent_run.created_at DESC LIMIT 1";

/// The active run (`issue.py:1386-1402`): the newest run in one of the
/// [`ACTIVE_RUN_STATUSES`]. Same projection and joins as
/// [`LATEST_AGENT_RUN_SQL`]; the `IN` list carries the verbatim values.
pub const ACTIVE_AGENT_RUN_SQL: &str = "SELECT agent_run.id, agent_run.status, agent_run.executor_kind, agent_run.queue_position, agent_run.runner_id, agent_run.created_at, agent_run.assigned_at, agent_run.started_at, agent_run.ended_at, agent_run.done_payload, agent_run.error, agent_run.error_code, agent_run.llm_model, agent_run.input_tokens, agent_run.output_tokens, agent_run.total_tokens, runner.name, runner_live_state.observed_run_id, runner_live_state.last_event_at, runner_live_state.last_event_kind, runner_live_state.last_event_summary, runner_live_state.agent_pid, runner_live_state.agent_subprocess_alive, runner_live_state.approvals_pending, runner_live_state.usage, runner_live_state.llm_model, runner_live_state.turn_count, runner_live_state.updated_at FROM agent_run LEFT JOIN runner ON runner.id = agent_run.runner_id LEFT JOIN runner_live_state ON runner_live_state.runner_id = runner.id WHERE agent_run.work_item_id = $1 AND agent_run.status IN ('queued', 'assigned', 'waiting_for_worktree', 'running', 'cancel_requested', 'awaiting_approval', 'awaiting_reauth', 'paused_awaiting_input') ORDER BY agent_run.created_at DESC LIMIT 1";

/// `runs.count()` (`issue.py:1412`): every run for the issue, all statuses,
/// no join. `$1` is the issue id.
pub const AGENT_RUN_COUNT_SQL: &str = "SELECT COUNT(*) FROM agent_run WHERE work_item_id = $1";

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::dispatch::AgentRunStatus;
    use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker as TickerRowCheck;

    fn serialized_keys<T: serde::Serialize>(value: &T) -> Vec<String> {
        let rendered = serde_json::to_string(value).expect("serializes");
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut chars = rendered.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                }
                '"' if depth == 1 => {
                    let mut key = String::new();
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next == '"' {
                            break;
                        }
                        key.push(next);
                    }
                    if chars.peek() == Some(&':') {
                        keys.push(key);
                    }
                }
                _ => {}
            }
        }
        keys
    }

    fn const_keys<const N: usize>(fields: &[&str; N]) -> Vec<String> {
        fields.iter().map(|name| name.to_string()).collect()
    }

    const ISSUE: &str = "11111111-1111-1111-1111-111111111111";
    const PROJ: &str = "22222222-2222-2222-2222-222222222222";
    const STATE: &str = "33333333-3333-3333-3333-333333333333";
    const USER: &str = "44444444-4444-4444-4444-444444444444";
    const RUN: &str = "55555555-5555-5555-5555-555555555555";
    const RUNNER: &str = "66666666-6666-6666-6666-666666666666";

    fn base_row() -> IssueDetailBaseRow<'static> {
        IssueDetailBaseRow {
            id: ISSUE,
            name: "Fix the ticker",
            state_id: Some(STATE),
            sort_order: 65535.0,
            completed_at: None,
            estimate_point: None,
            priority: "high",
            complexity_score: 4,
            start_date: Some("2026-10-01"),
            target_date: None,
            sequence_id: 639,
            project_id: PROJ,
            parent_id: None,
            cycle_id: None,
            assigned_pod_id: None,
            agent_executor: None,
            module_ids: vec![],
            label_ids: vec![],
            assignee_ids: vec![USER],
            sub_issues_count: 2,
            created_at: "2026-10-02T21:42:34.824827Z",
            updated_at: "2026-10-04T03:57:29.850339Z",
            created_by: Some(USER),
            updated_by: None,
            attachment_count: 0,
            link_count: 1,
            is_draft: false,
            archived_at: None,
            is_synced: false,
        }
    }

    fn ticker_row(used: i32, granted: i32, waited: i32) -> IssueAgentTicker {
        let stamp = chrono::DateTime::from_timestamp(1_759_421_200, 0).expect("stamp");
        let tick = chrono::DateTime::from_timestamp(1_759_391_200, 123_000_000).expect("tick");
        IssueAgentTicker {
            id: uuid::Uuid::nil(),
            created_at: stamp,
            updated_at: stamp,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            issue_id: uuid::Uuid::nil(),
            used,
            granted,
            waited,
            user_disabled: false,
            next_run_at: Some(stamp),
            last_tick_at: Some(tick),
            enabled: true,
            disarm_reason: String::new(),
            pending_entry: true,
            pending_entry_free: false,
            pending_entry_actor_id: None,
            pending_entry_trigger: String::new(),
            resume_parent_run_id: None,
        }
    }

    fn policy() -> ProjectClockPolicy {
        ProjectClockPolicy {
            agent_ticking_enabled: None,
            agent_default_max_ticks: Some(10),
            agent_default_interval_seconds: Some(7200),
            agent_review_default_interval_seconds: None,
            agent_test_default_interval_seconds: None,
        }
    }

    fn live_row<'a>(
        observed: Option<&'a str>,
        usage: &'a serde_json::Value,
    ) -> AgentLiveStateRow<'a> {
        AgentLiveStateRow {
            observed_run_id: observed,
            last_event_at: Some("2026-10-03T12:00:00+00:00"),
            last_event_kind: Some("tick"),
            last_event_summary: Some("working"),
            agent_pid: Some(4242),
            agent_subprocess_alive: Some(true),
            approvals_pending: Some(0),
            usage,
            llm_model: Some("m"),
            turn_count: Some(3),
            updated_at: "2026-10-03T12:00:01+00:00",
        }
    }

    fn run_row<'a>(
        id: &'a str,
        status: &'a str,
        live_state: Option<AgentLiveStateRow<'a>>,
    ) -> AgentRunDetailRow<'a> {
        AgentRunDetailRow {
            id,
            status,
            executor_kind: "local_runner",
            queue_position: None,
            runner_id: Some(RUNNER),
            runner_name: Some("r1"),
            created_at: "2026-10-03T10:00:00+00:00",
            assigned_at: Some("2026-10-03T10:00:01+00:00"),
            started_at: Some("2026-10-03T10:00:02+00:00"),
            ended_at: None,
            done_payload: None,
            error: "",
            error_code: "",
            llm_model: "m",
            input_tokens: Some(100),
            output_tokens: Some(20),
            total_tokens: Some(120),
            live_state,
        }
    }

    fn empty_summary() -> RelationsSummary {
        crate::orchestration::blockers::relations_summary(&[], &[], false)
    }

    #[test]
    fn base_fields_match_wire_order() {
        // TRACE: issue.py:1063-1093 (29 keys).
        assert_eq!(
            serialized_keys(&issue_detail_base_to_representation(&base_row())),
            const_keys(&ISSUE_DETAIL_BASE_FIELDS)
        );
    }

    #[test]
    fn base_replays_bytes() {
        // TRACE: issue.py:1039-1113; attname ReadOnlyFields; DRF datetimes.
        assert_eq!(
            serde_json::to_string(&issue_detail_base_to_representation(&base_row()))
                .expect("serializes"),
            "{\"id\":\"11111111-1111-1111-1111-111111111111\",\"name\":\"Fix the ticker\",\
             \"state_id\":\"33333333-3333-3333-3333-333333333333\",\"sort_order\":65535.0,\
             \"completed_at\":null,\"estimate_point\":null,\"priority\":\"high\",\
             \"complexity_score\":4,\"start_date\":\"2026-10-01\",\"target_date\":null,\
             \"sequence_id\":639,\"project_id\":\"22222222-2222-2222-2222-222222222222\",\
             \"parent_id\":null,\"cycle_id\":null,\"assigned_pod_id\":null,\
             \"agent_executor\":null,\"module_ids\":[],\"label_ids\":[],\
             \"assignee_ids\":[\"44444444-4444-4444-4444-444444444444\"],\
             \"sub_issues_count\":2,\"created_at\":\"2026-10-02T21:42:34.824827Z\",\
             \"updated_at\":\"2026-10-04T03:57:29.850339Z\",\
             \"created_by\":\"44444444-4444-4444-4444-444444444444\",\"updated_by\":null,\
             \"attachment_count\":0,\"link_count\":1,\"is_draft\":false,\
             \"archived_at\":null,\"is_synced\":false}",
        );
    }

    #[test]
    fn detail_identifier_has_36_keys() {
        // TRACE: issue.py:1237-1245; base.py:1436-1445 (is_intake annotated).
        let summary = empty_summary();
        let row = IssueDetailRow {
            base: base_row(),
            description_html: "<p></p>",
            is_subscribed: true,
            is_intake: Some(false),
            ticker: None,
            latest_run: None,
            active_run: None,
            run_count: 0,
            blockers: &summary,
        };
        assert_eq!(
            serialized_keys(&issue_detail_to_representation(&row)),
            const_keys(&ISSUE_DETAIL_FIELDS)
        );
    }

    #[test]
    fn detail_retrieve_omits_is_intake() {
        // TRACE: the ported SkipField bug — retrieve (:486-618) and archive
        // retrieve (:222-255) do not annotate is_intake, so the key is
        // absent (35 keys), while identifier renders it (even false).
        let summary = empty_summary();
        let row = IssueDetailRow {
            base: base_row(),
            description_html: "<p></p>",
            is_subscribed: false,
            is_intake: None,
            ticker: None,
            latest_run: None,
            active_run: None,
            run_count: 0,
            blockers: &summary,
        };
        let view = issue_detail_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            const_keys(&ISSUE_DETAIL_RETRIEVE_FIELDS)
        );
        let bytes = serde_json::to_string(&view).expect("serializes");
        assert!(!bytes.contains("is_intake"), "{bytes}");
        assert!(bytes.contains("\"agent_ticker\":null"), "{bytes}");
        assert!(bytes.contains("\"agent_status\":null"), "{bytes}");
    }

    #[test]
    fn ticker_replays_bytes() {
        // TRACE: issue.py:1298-1317; pool 10 + granted 10 + waited 1 = 21;
        // remaining 16; millis-exact micros keep six digits.
        let ticker = ticker_row(5, 10, 1);
        let policy = policy();
        let state = StateRef {
            group: "started",
            name: "In Progress",
        };
        assert_eq!(
            serde_json::to_string(&agent_ticker_to_representation(
                &ticker,
                &policy,
                Some(&state)
            ))
            .expect("serializes"),
            "{\"enabled\":true,\"user_disabled\":false,\"used\":5,\"tick_count\":5,\
             \"granted\":10,\"waited\":1,\"max_ticks\":21,\"remaining\":16,\
             \"interval_seconds\":7200,\
             \"next_run_at\":\"2025-10-02T16:06:40+00:00\",\
             \"last_tick_at\":\"2025-10-02T07:46:40.123000+00:00\",\
             \"disarm_reason\":\"\",\"pending_entry\":true,\"can_re_tick\":false}",
        );
    }

    #[test]
    fn ticker_can_re_tick_matrix() {
        // TRACE: issue.py:1296 — (ticking or paused) and cap_reached.
        let policy = policy();
        let ticking = StateRef {
            group: "started",
            name: "In Progress",
        };
        let paused = StateRef {
            group: "started",
            name: "Paused",
        };
        let other = StateRef {
            group: "backlog",
            name: "Backlog",
        };
        let capped = ticker_row(10, 0, 0);
        let open = ticker_row(5, 0, 0);
        assert!(agent_ticker_to_representation(&capped, &policy, Some(&ticking)).can_re_tick);
        assert!(agent_ticker_to_representation(&capped, &policy, Some(&paused)).can_re_tick);
        assert!(!agent_ticker_to_representation(&capped, &policy, Some(&other)).can_re_tick);
        assert!(!agent_ticker_to_representation(&capped, &policy, None).can_re_tick);
        assert!(!agent_ticker_to_representation(&open, &policy, Some(&ticking)).can_re_tick);
    }

    #[test]
    fn run_replays_bytes_with_diagnostic_and_live_state() {
        // TRACE: issue.py:1352-1371; unknown-error fallback; canonical usage
        // keys; observed id equal to the run id keeps the block.
        let usage = serde_json::json!({"input": 7, "output": 8, "total": 15});
        let payload = serde_json::json!({"signal": "done"});
        let mut row = run_row(RUN, "failed", Some(live_row(Some(RUN), &usage)));
        row.done_payload = Some(&payload);
        row.error = "boom\nsecond line";
        row.error_code = "E1";
        let view = agent_run_to_representation(Some(&row), true).expect("renders");
        assert_eq!(serialized_keys(&view), const_keys(&AGENT_RUN_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"55555555-5555-5555-5555-555555555555\",\"status\":\"failed\",\
             \"executor_kind\":\"local_runner\",\"queue_position\":null,\
             \"runner\":\"66666666-6666-6666-6666-666666666666\",\"runner_name\":\"r1\",\
             \"created_at\":\"2026-10-03T10:00:00+00:00\",\
             \"assigned_at\":\"2026-10-03T10:00:01+00:00\",\
             \"started_at\":\"2026-10-03T10:00:02+00:00\",\"ended_at\":null,\
             \"done_payload\":{\"signal\":\"done\"},\"error\":\"boom\\nsecond line\",\
             \"error_code\":\"E1\",\"error_diagnostic\":{\"source\":\"unknown\",\
             \"source_label\":\"Unknown\",\"kind\":\"unknown\",\"summary\":\"boom\",\
             \"action\":\"\"},\"llm_model\":\"m\",\"input_tokens\":100,\
             \"output_tokens\":20,\"total_tokens\":120,\
             \"live_state\":{\"observed_run_id\":\"55555555-5555-5555-5555-555555555555\",\
             \"last_event_at\":\"2026-10-03T12:00:00+00:00\",\"last_event_kind\":\"tick\",\
             \"last_event_summary\":\"working\",\"agent_pid\":4242,\
             \"agent_subprocess_alive\":true,\"approvals_pending\":0,\
             \"input_tokens\":7,\"output_tokens\":8,\"total_tokens\":15,\
             \"llm_model\":\"m\",\"turn_count\":3,\
             \"updated_at\":\"2026-10-03T12:00:01+00:00\"}}",
        );
    }

    #[test]
    fn run_none_renders_none() {
        // TRACE: issue.py:1340-1341.
        assert_eq!(agent_run_to_representation(None, true), None);
        assert_eq!(agent_run_to_representation(None, false), None);
    }

    #[test]
    fn live_state_observed_guard() {
        // TRACE: issue.py:1343-1350 — None observed keeps the block; a
        // mismatch drops it; no runner or no flag drops it too.
        let usage = serde_json::json!({});
        let keep_none = run_row(RUN, "running", Some(live_row(None, &usage)));
        let view = agent_run_to_representation(Some(&keep_none), true).expect("renders");
        assert!(view.live_state.is_some());

        let stale = run_row(
            RUN,
            "running",
            Some(live_row(
                Some("99999999-9999-9999-9999-999999999999"),
                &usage,
            )),
        );
        let view = agent_run_to_representation(Some(&stale), true).expect("renders");
        assert!(view.live_state.is_none());

        let mut no_runner = run_row(RUN, "running", Some(live_row(Some(RUN), &usage)));
        no_runner.runner_id = None;
        let view = agent_run_to_representation(Some(&no_runner), true).expect("renders");
        assert!(view.live_state.is_none());
        assert!(view.runner.is_none());
        assert!(view.runner_name.is_none());

        let view = agent_run_to_representation(Some(&keep_none), false).expect("renders");
        assert!(view.live_state.is_none());
    }

    #[test]
    fn status_none_rule_and_live_state_split() {
        // TRACE: issue.py:1404-1405 (None iff no ticker and no latest);
        // :1410-1411 (latest carries live state iff no active run).
        let usage = serde_json::json!({});
        assert!(agent_status_to_representation(None, None, None, 0).is_none());

        let latest = run_row(RUN, "completed", Some(live_row(Some(RUN), &usage)));
        let status = agent_status_to_representation(None, None, Some(&latest), 1).expect("renders");
        assert_eq!(serialized_keys(&status), const_keys(&AGENT_STATUS_FIELDS));
        assert!(status.ticker.is_none());
        assert!(status.active_run.is_none());
        let latest_view = status.latest_run.expect("latest renders");
        assert!(latest_view.live_state.is_some());
        assert_eq!(status.run_count, 1);

        let active = run_row(RUN, "running", Some(live_row(Some(RUN), &usage)));
        let status =
            agent_status_to_representation(None, Some(&active), Some(&latest), 2).expect("renders");
        assert!(status
            .active_run
            .expect("active renders")
            .live_state
            .is_some());
        assert!(status
            .latest_run
            .expect("latest renders")
            .live_state
            .is_none());
    }

    #[test]
    fn blocker_selectors_read_one_summary() {
        // TRACE: issue.py:1250-1266 — both fields select out of one call.
        let summary = crate::orchestration::blockers::relations_summary(&[], &[], true);
        assert!(relations_summary_field(&summary).blocked_by.is_empty());
        assert!(relations_summary_field(&summary).blocking.is_empty());
        assert!(has_open_blockers_field(&summary));
        assert!(!has_open_blockers_field(&empty_summary()));
    }

    #[test]
    fn diagnostic_wiring_matches_fixture_goldens() {
        // TRACE: FX-ISS-02 classify_run_error goldens, via the merged port.
        assert_eq!(classify_run_error(Some("")), None);
        assert_eq!(classify_run_error(Some("   \n  ")), None);
        let auth = classify_run_error(Some(
            "401 authentication_failed\nAI agent: Codex auth appears expired.",
        ))
        .expect("classifies");
        assert_eq!(
            serde_json::to_string(&auth).expect("serializes"),
            "{\"source\":\"agent\",\"source_label\":\"Codex\",\
             \"kind\":\"agent_authentication\",\
             \"summary\":\"401 authentication_failed\",\
             \"action\":\"Re-authenticate Codex on the runner machine, then restart the Pi Dash runner.\"}",
        );
        assert_eq!(serialized_keys(&auth), const_keys(&ERROR_DIAGNOSTIC_FIELDS));
        let model = classify_run_error(Some("selected model may not exist")).expect("classifies");
        assert_eq!(
            serde_json::to_string(&model).expect("serializes"),
            "{\"source\":\"agent\",\"source_label\":\"Agent CLI\",\
             \"kind\":\"agent_model_access\",\"summary\":\"selected model may not exist\",\
             \"action\":\"Choose a model the agent account can access, then retry the run.\"}",
        );
        let unknown = classify_run_error(Some("weird failure")).expect("classifies");
        assert_eq!(
            serde_json::to_string(&unknown).expect("serializes"),
            "{\"source\":\"unknown\",\"source_label\":\"Unknown\",\"kind\":\"unknown\",\
             \"summary\":\"weird failure\",\"action\":\"\"}",
        );
    }

    #[test]
    fn lite_replays_bytes() {
        // TRACE: issue.py:1219-1225.
        let row = IssueLiteRow {
            id: ISSUE,
            sequence_id: 639,
            project_id: PROJ,
        };
        let view = issue_lite_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&ISSUE_LITE_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"11111111-1111-1111-1111-111111111111\",\"sequence_id\":639,\
             \"project_id\":\"22222222-2222-2222-2222-222222222222\"}",
        );
    }

    fn actor_row() -> UserLiteRow<'static> {
        UserLiteRow {
            id: USER,
            first_name: "Ada",
            last_name: "L",
            avatar: "",
            avatar_url: None,
            is_bot: false,
            display_name: "Ada L",
        }
    }

    #[test]
    fn reaction_wire_order_is_probed_all_order() {
        // TRACE: issue.py:900-906 (__all__ + declared nest); order probed
        // with a lookalike hierarchy on DRF.
        let row = AppIssueReactionRow {
            id: "77777777-7777-7777-7777-777777777777",
            actor_detail: actor_row(),
            created_at: "2026-10-03T10:00:00Z",
            updated_at: "2026-10-03T10:00:01Z",
            deleted_at: None,
            reaction: "+1",
            created_by: Some(USER),
            updated_by: None,
            project: PROJ,
            workspace: "88888888-8888-8888-8888-888888888888",
            actor: USER,
            issue: ISSUE,
        };
        let view = app_issue_reaction_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            const_keys(&APP_ISSUE_REACTION_FIELDS)
        );
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"77777777-7777-7777-7777-777777777777\",\
             \"actor_detail\":{\"id\":\"44444444-4444-4444-4444-444444444444\",\
             \"first_name\":\"Ada\",\"last_name\":\"L\",\"avatar\":\"\",\
             \"avatar_url\":null,\"is_bot\":false,\"display_name\":\"Ada L\"},\
             \"created_at\":\"2026-10-03T10:00:00Z\",\
             \"updated_at\":\"2026-10-03T10:00:01Z\",\"deleted_at\":null,\
             \"reaction\":\"+1\",\
             \"created_by\":\"44444444-4444-4444-4444-444444444444\",\
             \"updated_by\":null,\
             \"project\":\"22222222-2222-2222-2222-222222222222\",\
             \"workspace\":\"88888888-8888-8888-8888-888888888888\",\
             \"actor\":\"44444444-4444-4444-4444-444444444444\",\
             \"issue\":\"11111111-1111-1111-1111-111111111111\"}",
        );
    }

    #[test]
    fn vote_replays_bytes() {
        // TRACE: issue.py:939-945.
        let row = AppIssueVoteRow {
            issue: ISSUE,
            vote: 1,
            workspace: "88888888-8888-8888-8888-888888888888",
            project: PROJ,
            actor: USER,
            actor_detail: actor_row(),
        };
        let view = app_issue_vote_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&APP_ISSUE_VOTE_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"issue\":\"11111111-1111-1111-1111-111111111111\",\"vote\":1,\
             \"workspace\":\"88888888-8888-8888-8888-888888888888\",\
             \"project\":\"22222222-2222-2222-2222-222222222222\",\
             \"actor\":\"44444444-4444-4444-4444-444444444444\",\
             \"actor_detail\":{\"id\":\"44444444-4444-4444-4444-444444444444\",\
             \"first_name\":\"Ada\",\"last_name\":\"L\",\"avatar\":\"\",\
             \"avatar_url\":null,\"is_bot\":false,\"display_name\":\"Ada L\"}}",
        );
    }

    #[test]
    fn public_replays_bytes() {
        // TRACE: issue.py:1415-1440 (ported as-is; no view references it).
        let logo = serde_json::json!({"icon": "bug"});
        let row = IssuePublicRow {
            id: ISSUE,
            name: "Public bug",
            description_html: "<p>hi</p>",
            sequence_id: 7,
            state: Some(STATE),
            state_detail: StateLiteRow {
                id: STATE,
                name: "Todo",
                color: "#ccc",
                group: "backlog",
            },
            project: PROJ,
            project_detail: ProjectLiteRow {
                id: PROJ,
                identifier: "P",
                name: "Pi",
                cover_image: None,
                cover_image_url: None,
                logo_props: &logo,
                description: "",
                is_default: false,
            },
            workspace: "88888888-8888-8888-8888-888888888888",
            priority: "none",
            target_date: None,
            reactions: vec![],
            votes: vec![],
        };
        let view = issue_public_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&ISSUE_PUBLIC_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            "{\"id\":\"11111111-1111-1111-1111-111111111111\",\"name\":\"Public bug\",\
             \"description_html\":\"<p>hi</p>\",\"sequence_id\":7,\
             \"state\":\"33333333-3333-3333-3333-333333333333\",\
             \"state_detail\":{\"id\":\"33333333-3333-3333-3333-333333333333\",\
             \"name\":\"Todo\",\"color\":\"#ccc\",\"group\":\"backlog\"},\
             \"project\":\"22222222-2222-2222-2222-222222222222\",\
             \"project_detail\":{\"id\":\"22222222-2222-2222-2222-222222222222\",\
             \"identifier\":\"P\",\"name\":\"Pi\",\"cover_image\":null,\
             \"cover_image_url\":null,\"logo_props\":{\"icon\":\"bug\"},\
             \"description\":\"\",\"is_default\":false},\
             \"workspace\":\"88888888-8888-8888-8888-888888888888\",\
             \"priority\":\"none\",\"target_date\":null,\"reactions\":[],\"votes\":[]}",
        );
    }

    #[test]
    fn run_sql_pins_tables_joins_and_predicates() {
        // TRACE: issue.py:1384-1385 (latest), :1386-1402 (active),
        // :1412 (count); runner/models.py tables.
        for sql in [LATEST_AGENT_RUN_SQL, ACTIVE_AGENT_RUN_SQL] {
            assert!(sql.starts_with("SELECT agent_run.id"), "{sql}");
            assert!(
                sql.contains(
                    "FROM agent_run LEFT JOIN runner ON runner.id = agent_run.runner_id \
                     LEFT JOIN runner_live_state ON runner_live_state.runner_id = runner.id"
                ),
                "{sql}"
            );
            assert!(sql.contains("agent_run.work_item_id = $1"), "{sql}");
            assert!(sql.contains("runner_live_state.usage"), "{sql}");
            assert!(!sql.contains("deleted_at"), "{sql}");
            assert!(
                sql.ends_with("ORDER BY agent_run.created_at DESC LIMIT 1"),
                "{sql}"
            );
        }
        assert!(
            !LATEST_AGENT_RUN_SQL.contains("agent_run.status IN"),
            "latest is unfiltered"
        );
        for status in ACTIVE_RUN_STATUSES {
            assert!(
                ACTIVE_AGENT_RUN_SQL.contains(&format!("'{status}'")),
                "active filters {status}"
            );
        }
        assert_eq!(
            AGENT_RUN_COUNT_SQL,
            "SELECT COUNT(*) FROM agent_run WHERE work_item_id = $1"
        );
    }

    #[test]
    fn active_statuses_match_merged_enum_values() {
        // TRACE: runner/models.py AgentRunStatus values via the merged
        // db port (FX-ISS-10 cites it; no SQL literal drift allowed).
        let expected = [
            AgentRunStatus::Queued,
            AgentRunStatus::Assigned,
            AgentRunStatus::WaitingForWorktree,
            AgentRunStatus::Running,
            AgentRunStatus::CancelRequested,
            AgentRunStatus::AwaitingApproval,
            AgentRunStatus::AwaitingReauth,
            AgentRunStatus::PausedAwaitingInput,
        ]
        .map(|status| status.value().to_string());
        let ours: Vec<String> = ACTIVE_RUN_STATUSES.iter().map(|s| s.to_string()).collect();
        assert_eq!(ours, expected);
    }

    #[test]
    fn datetime_helpers_match_python_and_drf() {
        // TRACE: issue.py:1247-1248 (isoformat) vs DRF auto fields (Z).
        let whole = chrono::DateTime::from_timestamp(1_759_421_200, 0).expect("stamp");
        assert_eq!(serialize_iso_datetime(whole), "2025-10-02T16:06:40+00:00");
        assert_eq!(serialize_drf_datetime(whole), "2025-10-02T16:06:40Z");
        let millis = chrono::DateTime::from_timestamp(1_759_421_200, 123_000_000).expect("stamp");
        assert_eq!(
            serialize_iso_datetime(millis),
            "2025-10-02T16:06:40.123000+00:00"
        );
        assert_eq!(
            serialize_drf_datetime(millis),
            "2025-10-02T16:06:40.123000Z"
        );
        let micros = chrono::DateTime::from_timestamp(1_759_421_200, 123_456_000).expect("stamp");
        assert_eq!(
            serialize_iso_datetime(micros),
            "2025-10-02T16:06:40.123456+00:00"
        );
    }

    #[test]
    fn live_state_tokens_delegate_to_merged_coercion() {
        // TRACE: runner/models.py:1510-1516 via flat_token_fields — garbage,
        // negatives and bools coerce to None.
        let usage = serde_json::json!({"input": 5, "output": "7", "total": -1});
        let row = live_row(None, &usage);
        let view = agent_live_state_to_representation(&row);
        assert_eq!(view.input_tokens, Some(5));
        assert_eq!(view.output_tokens, Some(7));
        assert_eq!(view.total_tokens, None);
        let junk = serde_json::json!({"input": true, "output": [1], "total": "x"});
        let row = live_row(None, &junk);
        let view = agent_live_state_to_representation(&row);
        assert_eq!(view.input_tokens, None);
        assert_eq!(view.output_tokens, None);
        assert_eq!(view.total_tokens, None);
    }

    #[test]
    fn ticker_struct_is_the_merged_db_shape() {
        // The ticker input is the merged D-10 struct itself (field parity
        // asserted at the type level by construction in ticker_row()).
        fn takes_merged(_: &TickerRowCheck) {}
        let ticker = ticker_row(0, 0, 0);
        takes_merged(&ticker);
    }
}
