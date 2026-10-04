#![forbid(unsafe_code)]

//! Matcher guards: status sets + legacy/preflight queries (D-14, stage 5).
//!
//! Port of the consts and the non-dispatch half of
//! `apps/api/pi_dash/runner/services/matcher.py`:
//!
//! * `HEARTBEAT_GRACE` (`:44`) → [`HEARTBEAT_GRACE_SECS`] +
//!   [`heartbeat_grace`] + [`alive_threshold`].
//! * `NON_TERMINAL_STATUSES` (`:54-67`) → [`NON_TERMINAL_STATUSES`].
//! * `BUSY_STATUSES` (`:70-81`) → [`BUSY_STATUSES`].
//! * `select_runner_for_run` (`:338-360`) →
//!   [`SELECT_RUNNER_FOR_RUN_SQL`].
//! * `can_register_another` (`:363-372`) → [`can_register_another`].
//! * `pod_has_runner_for_issue_principal` (`:375-448`) →
//!   [`is_managed_runner_issue`] + [`eligible_owner_ids`] +
//!   [`ISSUE_ASSIGNEE_IDS_SQL`] + [`POD_HAS_RUNNER_MANAGED_SQL`] +
//!   [`pod_has_runner_general_sql`].
//! * `count_active` (`:451-460`) → [`COUNT_ACTIVE_SQL`].
//! * `eligible_for_assignment` (`:463-465`) →
//!   [`eligible_for_assignment_predicate`].
//!
//! Out of scope here (sibling sub-issues): `select_runner_in_pod` /
//! `next_queued_run_for_pod` / `next_for_runner` / `drain_pod` /
//! `drain_for_runner` (`:89-303`, PIDASHCONV-552, which queries
//! against these consts) and `_build_assign_msg` (`:306-336`, the
//! shapes sub-issue).
//!
//! # Use, don't port
//!
//! * `AgentRunStatus` → `pidash_types::runner_runs` (whose docs name
//!   D-14 as a reuser); the sets below hold those variants.
//! * `RunnerStatus` / `RunnerProvisioning` values + `MAX_PER_USER`
//!   → `pidash_db::runner_enroll::columns::{enums, runner}`; the SQL
//!   consts inline the same literals and the tests pin them against
//!   those consts so the two cannot drift.
//! * `effective_executor_for_issue` → [`crate::dispatch`] (D-11);
//!   [`is_managed_runner_issue`] is the `:416` comparison only.
//!
//! # Translation notes
//!
//! * This crate has no database handle: SQL is text (Django shape —
//!   quoted identifiers, `%s` params rendered as Postgres `$N`),
//!   branch inputs arrive as caller-fetched facts, and the executing
//!   layer binds `$N` positionally. Fixed enum values are literals;
//!   caller-supplied ids/timestamps are `$N` params, each documented
//!   on its const.
//! * `alive_threshold` takes `now` as a parameter (the
//!   `mint_enrollment_token` precedent): Python evaluates
//!   `timezone.now()` per call.
//! * `owner_id__in=<set>` (`:441-443`) iterates a Python set, so
//!   Django's `IN` order is set-iteration order. Rust pins a
//!   deterministic order instead — run creator, issue creator, then
//!   through-table assignees in row order, deduplicated
//!   ([`eligible_owner_ids`]) — with identical membership (the D-15
//!   `IN`-from-set precedent: same semantics, stable text).
//! * `pod_has_runner_general_sql` returns `None` for an empty owner
//!   set: Python short-circuits (`:438-439`) and issues no query.
//! * `eligible_for_assignment` returns a `Q`, not SQL; the port is
//!   the equivalent `WHERE` fragment (the D-15
//!   `runner_visible_predicate` precedent).
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * Retired `WAITING_FOR_WORKTREE` stays in **both** sets
//!   (`:57-61`, `:72-75`, PDASHOSS01-137): historical rows must keep
//!   gating pod deletion and reporting busy. Ported as written.
//! * `select_runner_for_run` keeps the legacy owner-scoped semantics
//!   (`owner=run.owner`, `:349`) — the old access-gating rule,
//!   back-compat until Phase 3 migrates callers. Ported as written,
//!   not re-scoped to the pod.
//! * The legacy select carries **no** `DESKTOP_BUNDLED` exclusion
//!   (unlike `select_runner_in_pod` `:109`); the fixture SQL
//!   confirms the missing term. Ported as written.
//!
//! Fixture: `rust-api/fixtures/runner_sessions/fx-rses-07-matcher.json`
//! (FX-RSES-07). The `Runner` projection order is FX-RSES-01
//! (`fk_contract_column_lists.Runner`). Every section is replayed by
//! the `#[cfg(test)]` suite below.

use chrono::{DateTime, Duration, Utc};
use pidash_db::runner_enroll::columns::runner::MAX_PER_USER;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::AgentRunStatus;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Consts (`matcher.py:44-81`)
// ---------------------------------------------------------------------------

/// Heartbeat staleness window in seconds (`HEARTBEAT_GRACE`, `:44`).
pub const HEARTBEAT_GRACE_SECS: i64 = 90;

/// `HEARTBEAT_GRACE` as a duration (`timedelta(seconds=90)`, `:44`).
pub fn heartbeat_grace() -> Duration {
    Duration::seconds(HEARTBEAT_GRACE_SECS)
}

/// `timezone.now() - HEARTBEAT_GRACE` (`:345`): `now` arrives from the
/// caller (this crate performs no I/O) and the freshness bound goes
/// out as the `$1` threshold of [`SELECT_RUNNER_FOR_RUN_SQL`].
/// `last_heartbeat_at` is nullable; `NULL` rows fail the `>=`
/// comparison in both engines.
pub fn alive_threshold(now: DateTime<Utc>) -> DateTime<Utc> {
    now - heartbeat_grace()
}

/// Runs occupying a slot or queue position (`:54-67`), in
/// source-tuple order. Gates pod deletion; `PAUSED_AWAITING_INPUT`
/// is a member here (the run resumes on a comment) but not of
/// [`BUSY_STATUSES`] (the runner is free meanwhile). Retired
/// `WAITING_FOR_WORKTREE` is kept — see the ported-bugs note.
pub const NON_TERMINAL_STATUSES: [AgentRunStatus; 8] = [
    AgentRunStatus::Queued,
    AgentRunStatus::Assigned,
    AgentRunStatus::WaitingForWorktree,
    AgentRunStatus::Running,
    AgentRunStatus::CancelRequested,
    AgentRunStatus::AwaitingApproval,
    AgentRunStatus::AwaitingReauth,
    AgentRunStatus::PausedAwaitingInput,
];

/// Statuses marking a runner busy (`:70-81`), in source-tuple order.
/// Excludes `PAUSED_AWAITING_INPUT` (see [`NON_TERMINAL_STATUSES`]).
/// Retired `WAITING_FOR_WORKTREE` is kept — see the ported-bugs note.
pub const BUSY_STATUSES: [AgentRunStatus; 6] = [
    AgentRunStatus::Assigned,
    AgentRunStatus::WaitingForWorktree,
    AgentRunStatus::Running,
    AgentRunStatus::CancelRequested,
    AgentRunStatus::AwaitingApproval,
    AgentRunStatus::AwaitingReauth,
];

// ---------------------------------------------------------------------------
// Runner projection (FX-RSES-01 `fk_contract_column_lists.Runner`)
// ---------------------------------------------------------------------------

/// `runner` columns in Django `_meta` order — the `SELECT` projection
/// of [`SELECT_RUNNER_FOR_RUN_SQL`], pinned against FX-RSES-01 by the
/// tests below.
pub const RUNNER_COLUMNS: [&str; 30] = [
    "id",
    "owner_id",
    "workspace_id",
    "dev_machine_id",
    "pod_id",
    "name",
    "host_label",
    "provisioning",
    "visibility",
    "refresh_token_hash",
    "refresh_token_fingerprint",
    "refresh_token_generation",
    "previous_refresh_token_hash",
    "access_token_signing_key_version",
    "enrollment_token_hash",
    "enrollment_token_fingerprint",
    "enrolled_at",
    "capabilities",
    "status",
    "os",
    "arch",
    "runner_version",
    "dev_metadata",
    "protocol_version",
    "last_heartbeat_at",
    "free_worktrees",
    "created_at",
    "updated_at",
    "revoked_at",
    "revoked_reason",
];

// ---------------------------------------------------------------------------
// Legacy select (`matcher.py:338-360`)
// ---------------------------------------------------------------------------

/// `select_runner_for_run` (`:338-360`): legacy owner-scoped matcher.
///
/// `$1` is the [`alive_threshold`], `$2` the run owner's id
/// (`owner=run.owner`), `$3` the run's workspace id. `status` is the
/// `'online'` literal; the `NOT EXISTS` arm is the
/// `.exclude(agent_runs__status__in=BUSY_STATUSES)` reverse join
/// (`agent_run.runner_id`, alias `U1`, values in [`BUSY_STATUSES`]
/// order). The explicit `-last_heartbeat_at` order replaces
/// `Runner.Meta.ordering`, so `created_at` is absent (contrast the
/// `drain_for_runner` lock, which has no explicit order and keeps
/// both keys). `FOR UPDATE SKIP LOCKED`: call inside a transaction.
///
/// There is deliberately no `provisioning` term — see the
/// ported-bugs note.
pub const SELECT_RUNNER_FOR_RUN_SQL: &str = "SELECT \"runner\".\"id\", \"runner\".\"owner_id\", \"runner\".\"workspace_id\", \"runner\".\"dev_machine_id\", \"runner\".\"pod_id\", \"runner\".\"name\", \"runner\".\"host_label\", \"runner\".\"provisioning\", \"runner\".\"visibility\", \"runner\".\"refresh_token_hash\", \"runner\".\"refresh_token_fingerprint\", \"runner\".\"refresh_token_generation\", \"runner\".\"previous_refresh_token_hash\", \"runner\".\"access_token_signing_key_version\", \"runner\".\"enrollment_token_hash\", \"runner\".\"enrollment_token_fingerprint\", \"runner\".\"enrolled_at\", \"runner\".\"capabilities\", \"runner\".\"status\", \"runner\".\"os\", \"runner\".\"arch\", \"runner\".\"runner_version\", \"runner\".\"dev_metadata\", \"runner\".\"protocol_version\", \"runner\".\"last_heartbeat_at\", \"runner\".\"free_worktrees\", \"runner\".\"created_at\", \"runner\".\"updated_at\", \"runner\".\"revoked_at\", \"runner\".\"revoked_reason\" FROM \"runner\" WHERE (\"runner\".\"last_heartbeat_at\" >= $1 AND \"runner\".\"owner_id\" = $2 AND \"runner\".\"status\" = 'online' AND \"runner\".\"workspace_id\" = $3 AND NOT (EXISTS(SELECT 1 AS \"a\" FROM \"agent_run\" U1 WHERE (U1.\"status\" IN ('assigned', 'waiting_for_worktree', 'running', 'cancel_requested', 'awaiting_approval', 'awaiting_reauth') AND U1.\"runner_id\" = (\"runner\".\"id\")) LIMIT 1))) ORDER BY \"runner\".\"last_heartbeat_at\" DESC LIMIT 1 FOR UPDATE SKIP LOCKED";

// ---------------------------------------------------------------------------
// Cap pair (`matcher.py:363-372`, `:451-460`)
// ---------------------------------------------------------------------------

/// `count_active` (`:451-460`): manually-enrolled, non-revoked
/// runners this user holds in a workspace. `$1` is the owner id,
/// `$2` the workspace id. `REVOKED` and `DESKTOP_BUNDLED` are
/// excluded; every other status (including offline rows) counts —
/// transient state is ignored by design (`:364-371`).
pub const COUNT_ACTIVE_SQL: &str = "SELECT COUNT(*) AS \"__count\" FROM \"runner\" WHERE (\"runner\".\"owner_id\" = $1 AND \"runner\".\"workspace_id\" = $2 AND NOT (\"runner\".\"status\" = 'revoked') AND NOT (\"runner\".\"provisioning\" = 'desktop_bundled'))";

/// `can_register_another` (`:363-372`): `count_active(...) <
/// Runner.MAX_PER_USER`. `active_count` is the [`COUNT_ACTIVE_SQL`]
/// verdict; the cap is [`MAX_PER_USER`] (reused, not redefined).
/// Strict `<`: a count of exactly 5 refuses.
pub fn can_register_another(active_count: i64) -> bool {
    active_count < i64::from(MAX_PER_USER)
}

// ---------------------------------------------------------------------------
// Preflight (`matcher.py:375-448`)
// ---------------------------------------------------------------------------

/// The `:416` branch: `effective_executor_for_issue(issue) ==
/// AgentExecutorKind.MANAGED_RUNNER`. `issue_agent_executor` is the
/// per-issue override (`None` inherits) and
/// `project_default_agent_executor` the project's default — the two
/// inputs [`crate::dispatch::effective_executor_for_issue`] resolves
/// (empty override counts as missing, as Python's `or` does).
pub fn is_managed_runner_issue(
    issue_agent_executor: Option<&str>,
    project_default_agent_executor: &str,
) -> bool {
    crate::dispatch::effective_executor_for_issue(
        issue_agent_executor,
        project_default_agent_executor,
    ) == AgentExecutorKind::ManagedRunner.value()
}

/// `IssueAssignee.objects.filter(issue=issue).values_list("assignee_id",
/// flat=True)` (`:434-436`): live assignees through the soft-delete
/// manager — **not** the `Issue.assignees` M2M, which would report
/// every user ever assigned (`:388-393`). `$1` is the issue id. The
/// `deleted_at IS NULL` manager scope precedes the issue term
/// (captured order); `-created_at` is `IssueAssignee.Meta.ordering`.
pub const ISSUE_ASSIGNEE_IDS_SQL: &str = "SELECT \"issue_assignees\".\"assignee_id\" FROM \"issue_assignees\" WHERE (\"issue_assignees\".\"deleted_at\" IS NULL AND \"issue_assignees\".\"issue_id\" = $1) ORDER BY \"issue_assignees\".\"created_at\" DESC";

/// The `:429-437` owner set: run creator, issue creator, live
/// through-table assignees — `None`s dropped, deduplicated.
/// `assignee_ids` is the [`ISSUE_ASSIGNEE_IDS_SQL`] verdict in row
/// order (the column is non-null, so no `None` arrives from there).
/// Order is deterministic (creator, issue creator, assignees) where
/// Python iterates a set — same membership; see the translation
/// note. An empty return means `:438-439`: answer `false` and issue
/// no query.
pub fn eligible_owner_ids(
    run_creator_id: Option<Uuid>,
    issue_created_by_id: Option<Uuid>,
    assignee_ids: &[Uuid],
) -> Vec<Uuid> {
    let mut ids: Vec<Uuid> = Vec::new();
    for id in run_creator_id
        .into_iter()
        .chain(issue_created_by_id)
        .chain(assignee_ids.iter().copied())
    {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// The managed-runner narrow branch (`:419-427`): a desktop-bundled
/// runner owned by the run creator, in this pod, not revoked. `$1`
/// is the run creator id (`None` answers `false` with no query,
/// `:417-418`), `$2` the pod id. Transient status is ignored — a
/// closed laptop is not a structural failure (`:409-411`).
pub const POD_HAS_RUNNER_MANAGED_SQL: &str = "SELECT 1 AS \"a\" FROM \"runner\" WHERE (\"runner\".\"owner_id\" = $1 AND \"runner\".\"pod_id\" = $2 AND \"runner\".\"provisioning\" = 'desktop_bundled' AND NOT (\"runner\".\"status\" = 'revoked')) LIMIT 1";

/// The general branch (`:440-447`): a non-revoked, non-desktop runner
/// in this pod owned by anyone in [`eligible_owner_ids`].
/// `owner_count` is that set's length; owners bind `$1..=$n` in set
/// order and the pod binds `$n+1`. `None` for an empty set — Python
/// short-circuits at `:438-439` and issues no query.
pub fn pod_has_runner_general_sql(owner_count: usize) -> Option<String> {
    if owner_count == 0 {
        return None;
    }
    let params: Vec<String> = (1..=owner_count).map(|n| format!("${n}")).collect();
    Some(format!(
        "SELECT 1 AS \"a\" FROM \"runner\" WHERE (\"runner\".\"owner_id\" IN ({}) AND \"runner\".\"pod_id\" = ${} AND NOT (\"runner\".\"status\" = 'revoked') AND NOT (\"runner\".\"provisioning\" = 'desktop_bundled')) LIMIT 1",
        params.join(", "),
        owner_count + 1,
    ))
}

// ---------------------------------------------------------------------------
// Assignment predicate (`matcher.py:463-465`)
// ---------------------------------------------------------------------------

/// `eligible_for_assignment` (`:463-465`): `Q(pk=runner.pk,
/// status=RunnerStatus.ONLINE)` as a `WHERE` fragment.
/// `runner_param` is the already-rendered runner id (`$N` at
/// runtime, the quoted literal when replaying fixtures). Heartbeat
/// freshness and busy state are deliberately absent — a convenience
/// predicate for assertions/tests, ported as written.
pub fn eligible_for_assignment_predicate(runner_param: &str) -> String {
    format!(
        "(\"runner\".\"id\" = {p} AND \"runner\".\"status\" = 'online')",
        p = runner_param
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::runner_enroll::columns::enums::{
        RUNNER_PROVISIONING_DESKTOP_BUNDLED, RUNNER_STATUS_ONLINE, RUNNER_STATUS_REVOKED,
    };
    use serde_json::Value;

    static FX07: &str =
        include_str!("../../../../fixtures/runner_sessions/fx-rses-07-matcher.json");
    static FX01: &str = include_str!("../../../../fixtures/runner_sessions/fx-rses-01-models.json");

    fn fx07() -> Value {
        serde_json::from_str(FX07).expect("FX-RSES-07 parses")
    }

    fn fx01() -> Value {
        serde_json::from_str(FX01).expect("FX-RSES-01 parses")
    }

    fn str_list(value: &Value) -> Vec<&str> {
        value
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str"))
            .collect()
    }

    fn status_values(statuses: &[AgentRunStatus]) -> Vec<&str> {
        statuses.iter().map(AgentRunStatus::value).collect()
    }

    // -- consts ------------------------------------------------------------

    #[test]
    fn consts_replay_fixture() {
        let consts = &fx07()["consts"];
        assert_eq!(
            consts["HEARTBEAT_GRACE"].as_str(),
            Some("datetime.timedelta(seconds=90)")
        );
        assert_eq!(consts["HEARTBEAT_GRACE_seconds"].as_f64(), Some(90.0));
        assert_eq!(HEARTBEAT_GRACE_SECS, 90);
        assert_eq!(heartbeat_grace(), Duration::seconds(90));
        assert_eq!(
            status_values(&NON_TERMINAL_STATUSES),
            str_list(&consts["NON_TERMINAL_STATUSES"])
        );
        assert_eq!(NON_TERMINAL_STATUSES.len(), 8);
        assert!(NON_TERMINAL_STATUSES.contains(&AgentRunStatus::PausedAwaitingInput));
        assert_eq!(
            status_values(&BUSY_STATUSES),
            str_list(&consts["BUSY_STATUSES"])
        );
        assert_eq!(BUSY_STATUSES.len(), 6);
        assert!(!BUSY_STATUSES.contains(&AgentRunStatus::PausedAwaitingInput));
        // Retired member kept in both sets (PDASHOSS01-137).
        assert!(NON_TERMINAL_STATUSES.contains(&AgentRunStatus::WaitingForWorktree));
        assert!(BUSY_STATUSES.contains(&AgentRunStatus::WaitingForWorktree));
        assert_eq!(consts["Runner_MAX_PER_USER"].as_i64(), Some(5));
        assert_eq!(MAX_PER_USER, 5);
    }

    #[test]
    fn alive_threshold_subtracts_grace() {
        let now = DateTime::parse_from_rfc3339("2026-10-03T00:32:05Z")
            .expect("fixed now")
            .with_timezone(&Utc);
        assert_eq!(alive_threshold(now), now - Duration::seconds(90));
    }

    // -- runner projection -------------------------------------------------

    #[test]
    fn runner_columns_match_fx_rses_01() {
        let v = fx01();
        let fields = v["fk_contract_column_lists"]["Runner"]["fields"]
            .as_array()
            .expect("Runner fields");
        let expected: Vec<&str> = fields
            .iter()
            .map(|f| f["column"].as_str().expect("column"))
            .collect();
        assert_eq!(RUNNER_COLUMNS.to_vec(), expected);
        assert_eq!(RUNNER_COLUMNS.len(), 30);
    }

    #[test]
    fn legacy_select_projects_runner_columns() {
        let select_list = SELECT_RUNNER_FOR_RUN_SQL
            .strip_prefix("SELECT ")
            .expect("SELECT prefix")
            .split_once(" FROM \"runner\"")
            .expect("FROM runner")
            .0;
        let projected: Vec<&str> = select_list
            .split(", ")
            .map(|c| {
                c.strip_prefix("\"runner\".\"")
                    .expect("qualified")
                    .strip_suffix('"')
                    .expect("quoted")
            })
            .collect();
        assert_eq!(projected, RUNNER_COLUMNS.to_vec());
    }

    // -- legacy select -----------------------------------------------------

    #[test]
    fn legacy_select_sql_matches_django() {
        let v = fx07();
        let django = v["select_runner_for_run_legacy"]["sql"][0]
            .as_str()
            .expect("sql");
        let normalized = django
            .replace("'2026-10-03 00:32:05.032439+00:00'::timestamptz", "$1")
            .replace("'ce755a5b0c644e838f276218e639df62'::uuid", "$2")
            .replace("'af244c9a30a74b4a99b23a5c5ac09847'::uuid", "$3");
        assert_eq!(normalized, SELECT_RUNNER_FOR_RUN_SQL);
        // Captured WHERE term order: threshold, owner, status, workspace.
        let where_clause = SELECT_RUNNER_FOR_RUN_SQL
            .split_once(" WHERE (")
            .expect("WHERE")
            .1;
        let status_term = format!("\"runner\".\"status\" = '{RUNNER_STATUS_ONLINE}'");
        let terms = [
            "\"runner\".\"last_heartbeat_at\" >= $1",
            "\"runner\".\"owner_id\" = $2",
            status_term.as_str(),
            "\"runner\".\"workspace_id\" = $3",
        ];
        let mut cursor = 0;
        for term in terms {
            let pos = where_clause[cursor..]
                .find(term)
                .unwrap_or_else(|| panic!("term present: {term}"));
            cursor += pos + term.len();
        }
        // No provisioning term in the legacy WHERE (ported as written;
        // the projection still carries the column).
        let where_only = where_clause.split_once(" ORDER BY ").expect("ORDER BY").0;
        assert!(!where_only.contains("provisioning"));
        assert!(SELECT_RUNNER_FOR_RUN_SQL.ends_with(
            "ORDER BY \"runner\".\"last_heartbeat_at\" DESC LIMIT 1 FOR UPDATE SKIP LOCKED"
        ));
    }

    #[test]
    fn busy_in_list_follows_busy_statuses() {
        let list = BUSY_STATUSES
            .iter()
            .map(|s| format!("'{}'", s.value()))
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            SELECT_RUNNER_FOR_RUN_SQL.contains(&format!("U1.\"status\" IN ({list})")),
            "NOT EXISTS arm pins BUSY_STATUSES order"
        );
    }

    // -- cap pair ----------------------------------------------------------

    #[test]
    fn count_active_sql_matches_django() {
        let v = fx07();
        for key in ["sql", "can_sql"] {
            let django = v["count_active"][key][0].as_str().expect("sql");
            let normalized = django
                .replace("'ce755a5b0c644e838f276218e639df62'::uuid", "$1")
                .replace("'af244c9a30a74b4a99b23a5c5ac09847'::uuid", "$2");
            assert_eq!(normalized, COUNT_ACTIVE_SQL, "{key}");
        }
        assert!(COUNT_ACTIVE_SQL.contains(&format!(
            "NOT (\"runner\".\"status\" = '{RUNNER_STATUS_REVOKED}')"
        )));
        assert!(COUNT_ACTIVE_SQL.contains(&format!(
            "NOT (\"runner\".\"provisioning\" = '{RUNNER_PROVISIONING_DESKTOP_BUNDLED}')"
        )));
    }

    #[test]
    fn can_register_another_enforces_cap() {
        // Fixture: count 8 over the cap of 5 refuses; 7 after the
        // revoke still refuses.
        assert_eq!(fx07()["count_active"]["count"].as_i64(), Some(8));
        assert_eq!(
            fx07()["count_active"]["can_register_another"].as_bool(),
            Some(false)
        );
        assert_eq!(fx07()["count_active"]["after_revoke"].as_i64(), Some(7));
        assert!(!can_register_another(8));
        assert!(!can_register_another(7));
        assert!(!can_register_another(5));
        assert!(can_register_another(4));
        assert!(can_register_another(0));
    }

    // -- preflight ---------------------------------------------------------

    #[test]
    fn managed_branch_detection_matches_policy() {
        assert!(is_managed_runner_issue(
            Some("managed_runner"),
            "local_runner"
        ));
        assert!(is_managed_runner_issue(None, "managed_runner"));
        // Empty override counts as missing (Python `or`).
        assert!(is_managed_runner_issue(Some(""), "managed_runner"));
        assert!(!is_managed_runner_issue(
            Some("local_runner"),
            "local_runner"
        ));
        assert!(!is_managed_runner_issue(
            Some("cloud_agent"),
            "managed_runner"
        ));
        assert!(!is_managed_runner_issue(None, "local_runner"));
    }

    #[test]
    fn managed_exists_sql_matches_django() {
        let v = fx07();
        let django = v["pod_has_runner_for_issue_principal"]["managed_branch_owner"]["sql"][1]
            .as_str()
            .expect("sql");
        let normalized = django
            .replace("'ce755a5b0c644e838f276218e639df62'::uuid", "$1")
            .replace("'c63d5b13d56745de932a6e5e28568cfb'::uuid", "$2");
        assert_eq!(normalized, POD_HAS_RUNNER_MANAGED_SQL);
        assert_eq!(
            fx07()["pod_has_runner_for_issue_principal"]["managed_branch_owner"]["value"].as_bool(),
            Some(true)
        );
        // No-creator short-circuit: `false` with no query issued.
        let no_creator = &fx07()["pod_has_runner_for_issue_principal"]["managed_branch_no_creator"];
        assert_eq!(no_creator["value"].as_bool(), Some(false));
        assert!(no_creator["sql"].as_array().expect("sql").is_empty());
    }

    #[test]
    fn assignee_ids_sql_matches_django() {
        // Every general-branch case issues the same through-table read
        // modulo the issue literal: deleted_at scope first, -created_at
        // order, never the M2M join.
        let preflight = &fx07()["pod_has_runner_for_issue_principal"];
        for (case, issue) in [
            ("creator_owner", "'5e302935628f4fbba373bfbf12c3c979'::uuid"),
            ("stranger", "'5e302935628f4fbba373bfbf12c3c979'::uuid"),
            (
                "assignee_via_through_table",
                "'5e302935628f4fbba373bfbf12c3c979'::uuid",
            ),
            (
                "revoked_and_desktop_excluded",
                "'ede1990061f041679065acaafc76d943'::uuid",
            ),
        ] {
            let sqls = preflight[case]["sql"].as_array().expect("sql");
            let django = sqls
                .iter()
                .find_map(|s| {
                    s.as_str()
                        .filter(|t| t.contains("FROM \"issue_assignees\""))
                })
                .unwrap_or_else(|| panic!("assignee read in {case}"));
            assert_eq!(
                django.replace(issue, "$1"),
                ISSUE_ASSIGNEE_IDS_SQL,
                "{case}"
            );
        }
        assert!(!ISSUE_ASSIGNEE_IDS_SQL.contains("JOIN"));
    }

    #[test]
    fn eligible_owner_ids_collects_members() {
        let creator = Uuid::parse_str("ce755a5b0c644e838f276218e639df62").expect("uuid");
        let stranger = Uuid::parse_str("c66176a9c988414d9ac5dfe9432e3e57").expect("uuid");
        let assignee = Uuid::parse_str("4c3ff169f81648eba5ddb8d93681b765").expect("uuid");
        // Creator-only (fixture `creator_owner`: created_by null, no rows).
        assert_eq!(eligible_owner_ids(Some(creator), None, &[]), vec![creator]);
        // Creator + through-table assignee (fixture IN order).
        assert_eq!(
            eligible_owner_ids(Some(stranger), None, &[assignee]),
            vec![stranger, assignee]
        );
        // Issue creator fills in when the run creator is missing.
        assert_eq!(
            eligible_owner_ids(None, Some(creator), &[assignee]),
            vec![creator, assignee]
        );
        // `discard(None)` + dedupe: same id thrice collapses to one.
        assert_eq!(
            eligible_owner_ids(Some(creator), Some(creator), &[creator]),
            vec![creator]
        );
        // Empty set: `:438-439` answers `false` with no query.
        assert!(eligible_owner_ids(None, None, &[]).is_empty());
        assert_eq!(pod_has_runner_general_sql(0), None);
    }

    #[test]
    fn general_exists_sql_matches_django() {
        let preflight = &fx07()["pod_has_runner_for_issue_principal"];
        // Two-owner case: creator + through-table assignee.
        let django = preflight["assignee_via_through_table"]["sql"][1]
            .as_str()
            .expect("sql");
        let normalized = django
            .replace("'c66176a9c988414d9ac5dfe9432e3e57'::uuid", "$1")
            .replace("'4c3ff169f81648eba5ddb8d93681b765'::uuid", "$2")
            .replace("'c63d5b13d56745de932a6e5e28568cfb'::uuid", "$3");
        assert_eq!(pod_has_runner_general_sql(2).expect("some"), normalized);
        assert_eq!(
            preflight["assignee_via_through_table"]["value"].as_bool(),
            Some(true)
        );
        // Single-owner cases share one shape modulo the owner literal.
        for (case, owner) in [
            ("creator_owner", "'ce755a5b0c644e838f276218e639df62'::uuid"),
            ("stranger", "'c66176a9c988414d9ac5dfe9432e3e57'::uuid"),
            (
                "revoked_and_desktop_excluded",
                "'ce755a5b0c644e838f276218e639df62'::uuid",
            ),
        ] {
            let sqls = preflight[case]["sql"].as_array().expect("sql");
            let django = sqls
                .iter()
                .find_map(|s| {
                    s.as_str().filter(|t| {
                        t.contains("FROM \"runner\" WHERE (\"runner\".\"owner_id\" IN (")
                    })
                })
                .unwrap_or_else(|| panic!("general exists in {case}"));
            let normalized = django
                .replace(owner, "$1")
                .replace("'c63d5b13d56745de932a6e5e28568cfb'::uuid", "$2");
            assert_eq!(
                pod_has_runner_general_sql(1).expect("some"),
                normalized,
                "{case}"
            );
        }
        assert_eq!(preflight["creator_owner"]["value"].as_bool(), Some(true));
        assert_eq!(preflight["stranger"]["value"].as_bool(), Some(false));
        assert_eq!(
            preflight["soft_deleted_assignee"]["value"].as_bool(),
            Some(false)
        );
        assert_eq!(
            preflight["revoked_and_desktop_excluded"]["value"].as_bool(),
            Some(false)
        );
    }

    // -- assignment predicate ----------------------------------------------

    #[test]
    fn eligible_for_assignment_matches_q() {
        let v = fx07();
        let q = v["eligible_for_assignment"]["q"].as_str().expect("q");
        assert!(q.contains("RunnerStatus.ONLINE"), "Q pins ONLINE: {q}");
        let ours = eligible_for_assignment_predicate("'297ebdfd-8d3b-44ae-b295-964880f92013'");
        assert_eq!(
            ours,
            "(\"runner\".\"id\" = '297ebdfd-8d3b-44ae-b295-964880f92013' AND \"runner\".\"status\" = 'online')"
        );
        assert!(ours.contains(RUNNER_STATUS_ONLINE));
        assert_eq!(
            eligible_for_assignment_predicate("$1"),
            "(\"runner\".\"id\" = $1 AND \"runner\".\"status\" = 'online')"
        );
    }
}
