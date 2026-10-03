//! Run permission guards + pagination for D-15 runner_runs (L3).
//!
//! Ports `apps/api/pi_dash/runner/services/permissions.py:1-137` (the
//! predicate-SQL surface), `apps/api/pi_dash/runner/views/runs.py:67-122`
//! (`_parse_pagination`, `_can_view_run`, `_can_cancel_run`) and the
//! 404-not-403 denial those guards return (`runs.py:533/536/555/557/659/661`).
//!
//! # Use, don't port
//!
//! The boolean kernels already live in the foundation crates and are
//! reused, never re-implemented here:
//!
//! * `can_view` / `can_use` (runner + dev machine) and `can_manage_runner`
//!   (`permissions.py:57-80,126-137`) →
//!   [`pidash_auth::permissions::runner`]. The `#[cfg(test)]` suite below
//!   replays the FX-RUN-04 `runner_machine_matrix` through those kernels.
//! * `is_workspace_member` / `is_workspace_admin` (`core/permissions.py`,
//!   re-exported by `permissions.py:23-31`) →
//!   [`pidash_auth::permissions::membership`]. [`can_view_run`] takes them
//!   as caller-resolved facts, exactly like the intake guards precedent.
//! * `DEFAULT_PER_PAGE` / `MAX_PER_PAGE` (`runs.py:36-37`) →
//!   [`pidash_types::runner_runs::consts`].
//!
//! What this module adds is the D-15-owned surface the foundation kernels
//! do not carry: the `_can_view_run` involvement chain over caller facts,
//! the clamp-never-400 pagination, the two ORM-predicate SQL builders
//! (`runner_visible_to_user_q`, `filter_runs_usable_by_runner`), and the
//! exact denial body.
//!
//! # Out of scope (sibling sub-issues)
//!
//! * Chat `can_read` / `can_send` / `can_decide` (`services/chat.py:48-67`)
//!   ship with the L5 chat-service sub-issue (PIDASHCONV-537), not here.
//! * `ChatSendThrottle` enforcement ships with the L8 message-send handler
//!   (PIDASHCONV-543); its spec (scope `runner_chat_send`, POST-only) is
//!   pinned in FX-RUN-04 and needs no home here.
//!
//! # Fixture source of truth
//!
//! `rust-api/fixtures/runner_runs/fx-run-04-guards.golden.json` (FX-RUN-04:
//! `can_view_run` / `can_cancel_run` matrices, `runner_visible_to_user_q`
//! SQL, `runner_machine_matrix`, `filter_runs_usable_by_runner` SQL) and
//! the pagination vectors + denial bodies in
//! `fx-run-08-handlers-web.golden.json` (`list.page*`/`per*`, `detail`,
//! `cancel`, `release_pin`). Each `#[cfg(test)]` suite replays its section.
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * BUG-manage-admin-unreachable (`permissions.py:133-137`): the
//!   `or is_workspace_admin(...)` branch can never fire — the `can_view`
//!   gate admits only PRIVATE + owned rows, and PRIVATE rows return on the
//!   owner check first. The fixture pins it (`adminxmanage_weird_runner`
//!   is false); the foundation kernel ports the code as written.
//! * BUG-owner-before-gate (`runs.py:98-106`): the runner-owner grant runs
//!   before the private-runner gate, so an owner passes even for a (today
//!   nonexistent) non-private runner. Ported as written.
//! * `qs.none()` (`permissions.py:123`): `str()` raises `EmptyResultSet`
//!   and execution yields `[]`. [`runs_usable_by_runner_predicate`]
//!   returns `None` for that arm — the caller applies match-nothing.

use pidash_types::runner_runs::{DEFAULT_PER_PAGE, MAX_PER_PAGE};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Pagination (`runs.py:67-84`)
// ---------------------------------------------------------------------------

/// Resolve `page` (1-based) and `per_page` from raw query values.
///
/// Port of `_parse_pagination`: invalid or out-of-bounds values fall back
/// to safe defaults rather than erroring — `page` clamps to a minimum of
/// 1, `per_page` to `[1, MAX_PER_PAGE]` with a `DEFAULT_PER_PAGE` fallback.
/// Never 400s.
///
/// Each argument is the `QueryDict.get` value for its key (the *last* value
/// on repeats, `None` when absent); multi-value extraction stays the
/// caller's job, like the `params.rs` precedent.
pub fn parse_pagination(page_raw: Option<&str>, per_page_raw: Option<&str>) -> (i64, i64) {
    let page = py_int_or(page_raw, 1).max(1);
    // `page` is unbounded in Python (`max(1, huge)` stays huge); the i64
    // ceiling only bounds the OFFSET arithmetic the handler does next.
    let page = i64::try_from(page).unwrap_or(i64::MAX);
    let per_page =
        py_int_or(per_page_raw, DEFAULT_PER_PAGE as i128).clamp(1, MAX_PER_PAGE as i128) as i64;
    (page, per_page)
}

/// Port of the `_to_int` closure (`runs.py:75-79`): `int(value)` with a
/// `default` on `TypeError` (`None`) / `ValueError` (not an integer).
fn py_int_or(raw: Option<&str>, default: i128) -> i128 {
    match raw {
        None => default,
        Some(text) => py_int_saturated(text).unwrap_or(default),
    }
}

/// CPython `int(text)` for base-10 query values, `None` on `ValueError`.
///
/// Verified against CPython 3.9/3.12: surrounding whitespace stripped,
/// one optional sign, then digits with single underscores allowed only
/// between digits (`1_0` → 10; `_1`, `1_`, `1__2`, `+_1` all fail). Empty
/// after cleanup fails, as do decimals, exponents and hex (`12.0`, `1e3`,
/// `0x10`). Magnitudes past `i128` saturate — both consumers clamp into
/// range immediately (`page` floors at 1 with an i64 ceiling, `per_page`
/// clamps to `[1, 200]`), so saturation is unobservable.
///
/// Known divergence (shared with the merged `params.rs` precedent):
/// CPython also accepts non-ASCII decimal digits (`int('１２')` → 12);
/// this port reads ASCII digits only and falls back to the default there.
fn py_int_saturated(text: &str) -> Option<i128> {
    let trimmed = text.trim();
    let (negative, core) = match trimmed.strip_prefix(['+', '-']) {
        Some(rest) => (trimmed.starts_with('-'), rest),
        None => (false, trimmed),
    };
    if core.is_empty() {
        return None;
    }
    let bytes = core.as_bytes();
    let mut value: i128 = 0;
    let mut any_digit = false;
    let mut prev_was_digit = false;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'_' {
            // Underscores must sit between two digits (CPython rule).
            if !prev_was_digit {
                return None;
            }
            match bytes.get(i + 1) {
                Some(next) if next.is_ascii_digit() => {}
                _ => return None,
            }
            prev_was_digit = false;
            continue;
        }
        if !b.is_ascii_digit() {
            return None;
        }
        any_digit = true;
        prev_was_digit = true;
        value = value.saturating_mul(10).saturating_add((b - b'0') as i128);
    }
    if !any_digit {
        return None;
    }
    Some(if negative {
        value.saturating_neg()
    } else {
        value
    })
}

// ---------------------------------------------------------------------------
// Run view / cancel guards (`runs.py:87-122`)
// ---------------------------------------------------------------------------

/// The runner side of a [`can_view_run`] check: `run.runner_id` is set.
///
/// `visible_to_requester` is `can_view_runner(user, run.runner)`
/// (`runs.py:105`) — the caller computes it through the foundation kernel
/// (`pidash_auth::permissions::runner::can_view_runner`), which owns that
/// rule. `None` (no runner: queued local work, Cloud Agent runs) falls
/// through to the involvement grants below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunRunnerGate {
    /// `run.runner.owner_id == user.id` (`runs.py:98`).
    pub owned_by_requester: bool,
    /// `can_view_runner(user, run.runner)` (`runs.py:105`).
    pub visible_to_requester: bool,
}

/// Caller-resolved facts for [`can_view_run`].
///
/// Every fact is a value the caller's SQL already fetched, never a query —
/// exactly like the intake guards precedent. Tenant isolation (the
/// membership rows are live, non-soft-deleted `WorkspaceMember` rows for
/// this run's workspace) is the caller's job, as in Python.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunViewFacts {
    /// `is_workspace_member(user, run.workspace_id)` (`runs.py:94`).
    /// False for anonymous callers (`user` None / unauthenticated).
    pub is_workspace_member: bool,
    /// `run.created_by_id == user.id` (`runs.py:96`).
    pub created_by_requester: bool,
    /// The runner arm (`runs.py:98-106`); `None` when `run.runner_id` is
    /// unset.
    pub runner: Option<RunRunnerGate>,
    /// `run.work_item.created_by_id == user.id` (`runs.py:108`);
    /// `None` when `run.work_item_id` is unset.
    pub work_item_created_by_requester: Option<bool>,
    /// Live `IssueAssignee(issue_id, assignee_id)` row exists
    /// (`runs.py:115`, default manager — soft-deleted rows do not count,
    /// so un-assignment withdraws the grant). Only consulted when a work
    /// item is set.
    pub live_assignee: bool,
    /// `is_workspace_admin(user, run.workspace_id)` (`runs.py:117`).
    pub is_workspace_admin: bool,
}

/// View is allowed for the creator, the runner's owner, or a workspace
/// admin (`runs.py:87-117`).
///
/// Order is load-bearing and mirrors Python line for line:
/// membership-first (a removed member sees nothing, even as
/// `runner.owner`), creator, runner owner, the private-runner gate (a run
/// on someone else's private machine is visible only to creator and
/// runner owner — not to issue participants or workspace admins),
/// work-item creator, live assignee through the through-model (a plain
/// M2M join would ignore `deleted_at` and leave every past assignee able
/// to view and cancel), then workspace admin.
pub fn can_view_run(facts: &RunViewFacts) -> bool {
    if !facts.is_workspace_member {
        return false;
    }
    if facts.created_by_requester {
        return true;
    }
    if let Some(runner) = &facts.runner {
        if runner.owned_by_requester {
            return true;
        }
        if !runner.visible_to_requester {
            return false;
        }
    }
    if facts.work_item_created_by_requester == Some(true) {
        return true;
    }
    if facts.work_item_created_by_requester.is_some() && facts.live_assignee {
        return true;
    }
    facts.is_workspace_admin
}

/// Cancellation is permitted for the same set as view (`runs.py:120-122`).
pub fn can_cancel_run(facts: &RunViewFacts) -> bool {
    can_view_run(facts)
}

// ---------------------------------------------------------------------------
// Predicate SQL (`permissions.py:40-54,83-123`)
// ---------------------------------------------------------------------------

/// `runner_visible_to_user_q` for an authenticated user
/// (`permissions.py:49-54`): `(owner_id = P AND visibility = 0)`.
///
/// `owner_param` is the already-rendered owner value — `"$N"` at runtime,
/// the bare UUID literal when replaying `str(qs.query)` fixtures (Django
/// renders UUID params unquoted there). `runner_alias` is the runner
/// table (or its join alias); `prefix="runner__"` only adds the caller's
/// `INNER JOIN "runner" ON ("agent_run"."runner_id" = "runner"."id")`
/// — the predicate text is identical on the runner alias, as the
/// fixture's `authed_prefix` SQL shows.
pub fn runner_visible_predicate(owner_param: &str, runner_alias: &str) -> String {
    format!(
        r#"("{a}"."owner_id" = {p} AND "{a}"."visibility" = 0)"#,
        a = runner_alias,
        p = owner_param
    )
}

/// `runner_visible_to_user_q` for anonymous / `None` users
/// (`permissions.py:47-48`): `Q(pk__isnull=True)` matches nothing.
pub fn runner_visible_predicate_anon(runner_alias: &str) -> String {
    format!(r#""{a}"."id" IS NULL"#, a = runner_alias)
}

/// `filter_runs_usable_by_runner` (`permissions.py:83-123`): the `OR` over
/// the four usability arms for the runner owner's param.
///
/// Private runners consume work created by their owner, billed to their
/// owner, involving their owner (issue created by, or assigned through a
/// live `IssueAssignee` row — the through-model join with
/// `deleted_at IS NULL` in the same condition so both bind to one row),
/// or scheduler work authored by their owner. The two `EXISTS` subqueries
/// are Django's annotated filters rendered verbatim (join aliases `U0` /
/// `U2`, `SELECT 1 AS "a"`, `LIMIT 1`) with `%s` params as `owner_param`.
///
/// Cross-domain tables are inlined as predicate SQL only (no code calls
/// into other domains): `"issues"` ← `Issue`, `"issue_assignees"` ←
/// `IssueAssignee`, `"scheduler_bindings"` ← `SchedulerBinding`.
///
/// Non-private runners get `qs.none()` (`permissions.py:123`): `None`
/// here — the caller applies match-nothing (`str()` raises
/// `EmptyResultSet` in Python and execution yields `[]`).
pub fn runs_usable_by_runner_predicate(
    visibility: i32,
    owner_param: &str,
    run_alias: &str,
) -> Option<String> {
    if visibility != pidash_auth::permissions::runner::VISIBILITY_PRIVATE {
        return None;
    }
    let issue_exists = format!(
        r#"EXISTS(SELECT 1 AS "a" FROM "issues" U0 LEFT OUTER JOIN "issue_assignees" U2 ON (U0."id" = U2."issue_id") WHERE (U0."deleted_at" IS NULL AND U0."id" = ("{r}"."work_item_id") AND (U0."created_by_id" = {p} OR (U2."assignee_id" = {p} AND U2."deleted_at" IS NULL))) LIMIT 1)"#,
        r = run_alias,
        p = owner_param
    );
    let scheduler_exists = format!(
        r#"EXISTS(SELECT 1 AS "a" FROM "scheduler_bindings" U0 WHERE (U0."deleted_at" IS NULL AND U0."actor_id" = {p} AND U0."id" = ("{r}"."scheduler_binding_id")) LIMIT 1)"#,
        r = run_alias,
        p = owner_param
    );
    Some(format!(
        r#"("{r}"."created_by_id" = {p} OR "{r}"."owner_id" = {p} OR {issue} OR {sched})"#,
        r = run_alias,
        p = owner_param,
        issue = issue_exists,
        sched = scheduler_exists
    ))
}

// ---------------------------------------------------------------------------
// Denial bodies
// ---------------------------------------------------------------------------

/// An exact guard-denial response: HTTP status plus JSON body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denial {
    /// HTTP status code.
    pub status: u16,
    /// Byte-exact JSON body.
    pub body: Value,
}

/// Run lookup / guard denial (`runs.py:533/536/555/557/659/661`): 404, not
/// 403 — missing and forbidden both answer `{"error": "not found"}` so
/// callers cannot confirm run existence across workspaces. Covers the
/// detail, cancel and release-pin endpoints.
pub fn run_not_found() -> Denial {
    Denial {
        status: 404,
        body: json!({"error": "not found"}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_auth::permissions::runner as auth_runner;
    use pidash_types::{UserId, WorkspaceId};

    static FX04: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-04-guards.golden.json");
    static FX08: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-08-handlers-web.golden.json");

    fn fx04() -> Value {
        serde_json::from_str(FX04).expect("FX-RUN-04 parses")
    }

    fn fx08() -> Value {
        serde_json::from_str(FX08).expect("FX-RUN-08 parses")
    }

    // -- pagination --------------------------------------------------------

    #[test]
    fn pagination_replays_fixture_vectors() {
        let list = &fx08()["list"];
        // (page_raw, per_page_raw, fixture key carrying the resolved pair).
        for (page_raw, per_raw, key) in [
            (None, None, "default"),
            (Some("2"), Some("1"), "page2_per1"),
            (Some("0"), None, "page0"),
            (Some("abc"), None, "page_abc"),
            (None, Some("0"), "per0"),
            (None, Some("abc"), "per_abc"),
            (None, Some("500"), "per500"),
            (None, Some("-3"), "per_neg"),
            (Some("99"), None, "page99"),
        ] {
            let (page, per_page) = parse_pagination(page_raw, per_raw);
            let gold = &list[key];
            // `page2_per1` nests the pair under `body`; the rest are flat.
            let (want_page, want_per) = match gold.get("body") {
                Some(body) => (
                    body["page"].as_i64().expect("page"),
                    body["per_page"].as_i64().expect("per_page"),
                ),
                None => (
                    gold["page"].as_i64().expect("page"),
                    gold["per_page"].as_i64().expect("per_page"),
                ),
            };
            assert_eq!((page, per_page), (want_page, want_per), "vector {key}");
        }
    }

    #[test]
    fn pagination_clamps_never_400s() {
        // Python `int()` edges, verified against CPython: whitespace,
        // signs and single-between-digits underscores parse; everything
        // else falls back to the default.
        assert_eq!(parse_pagination(Some("  3  "), None), (3, 30));
        assert_eq!(parse_pagination(Some("+4"), Some("1_0")), (4, 10));
        assert_eq!(parse_pagination(Some("1__2"), Some("_1")), (1, 30));
        assert_eq!(parse_pagination(Some("1_"), Some("1.0")), (1, 30));
        assert_eq!(parse_pagination(Some(""), Some("0x10")), (1, 30));
        assert_eq!(parse_pagination(Some("1e3"), Some("  ")), (1, 30));
        // Bounds: page floors at 1 with no ceiling; per_page clamps both
        // sides. Unbounded magnitudes behave like CPython's `int()`.
        assert_eq!(parse_pagination(Some("-99"), Some("-99")), (1, 1));
        assert_eq!(
            parse_pagination(
                Some("99999999999999999999999999"),
                Some("99999999999999999999999999")
            ),
            (i64::MAX, 200)
        );
        assert_eq!(
            parse_pagination(
                Some("-99999999999999999999999999"),
                Some("-99999999999999999999999999")
            ),
            (1, 1)
        );
    }

    // -- run view / cancel matrices ----------------------------------------

    /// Fixture setup (`setup` + matrices): per-user membership/admin flags.
    fn user_flags(user: &str) -> (bool, bool) {
        match user {
            "admin" => (true, true),
            "owner" | "creator" | "assignee" | "stale" | "member" => (true, false),
            "outsider" | "anon" | "none" => (false, false),
            _ => panic!("unknown fixture user {user}"),
        }
    }

    /// Per-run shape: (created_by, runner_owner, work-item created_by,
    /// work-item live-assignee). `outsider_runner` carries no work item in
    /// any row that could distinguish it (the private gate decides first).
    fn run_shape(
        run: &str,
    ) -> (
        &'static str,
        Option<&'static str>,
        Option<(&'static str, &'static str)>,
    ) {
        match run {
            "private_runner" => ("creator", Some("owner"), Some(("creator", "assignee"))),
            "runnerless_issue" => ("creator", None, Some(("creator", "assignee"))),
            "runnerless_free" => ("creator", None, None),
            "outsider_runner" => ("creator", Some("outsider"), None),
            _ => panic!("unknown fixture run {run}"),
        }
    }

    fn facts_for(user: &str, run: &str) -> RunViewFacts {
        let (member, admin) = user_flags(user);
        let (created_by, runner_owner, work_item) = run_shape(run);
        let runner = runner_owner.map(|owner| {
            let owned = user == owner;
            RunRunnerGate {
                owned_by_requester: owned,
                // PRIVATE + owned, exactly as the foundation kernel
                // decides for an authenticated caller.
                visible_to_requester: member && owned,
            }
        });
        RunViewFacts {
            is_workspace_member: member,
            created_by_requester: user == created_by,
            runner,
            work_item_created_by_requester: work_item.map(|(by, _)| user == by),
            live_assignee: work_item
                .map(|(_, assignee)| user == assignee)
                .unwrap_or(false),
            is_workspace_admin: admin,
        }
    }

    #[test]
    fn can_view_run_replays_matrix() {
        let matrix = &fx04()["can_view_run"];
        let rows = matrix.as_object().expect("can_view_run map");
        assert_eq!(rows.len(), 30, "fixture row count pins coverage");
        for (key, want) in rows {
            let (user, run) = key.split_once('x').expect("userxrun key");
            let facts = facts_for(user, run);
            assert_eq!(
                can_view_run(&facts),
                want.as_bool().expect("bool"),
                "can_view_run {key}"
            );
        }
    }

    #[test]
    fn can_cancel_run_equals_view_everywhere() {
        let v = fx04();
        assert_eq!(v["cancel_equals_view_everywhere"].as_bool(), Some(true));
        let matrix = v["can_cancel_run"].as_object().expect("map");
        assert_eq!(matrix.len(), 28, "fixture row count pins coverage");
        for (key, want) in matrix {
            let (user, run) = key.split_once('x').expect("userxrun key");
            let facts = facts_for(user, run);
            assert_eq!(
                can_cancel_run(&facts),
                want.as_bool().expect("bool"),
                "can_cancel_run {key}"
            );
            assert_eq!(can_cancel_run(&facts), can_view_run(&facts));
        }
        // Anonymous callers are absent from the cancel map (present in the
        // view map as false); the delegate makes them false here too.
        for user in ["anon", "none"] {
            for run in [
                "private_runner",
                "runnerless_issue",
                "runnerless_free",
                "outsider_runner",
            ] {
                assert!(!can_cancel_run(&facts_for(user, run)));
            }
        }
    }

    // -- foundation boolean kernels vs the runner/machine matrix ----------

    #[test]
    fn foundation_kernels_replay_runner_machine_matrix() {
        let v = fx04();
        let matrix = &v["runner_machine_matrix"];
        // Fixture runner + machine are private, owned by `owner`.
        let scope = pidash_auth::TenantScope::new(WorkspaceId::from("ws"));
        for user in [
            "admin", "owner", "creator", "assignee", "stale", "member", "outsider", "anon", "none",
        ] {
            let authenticated = !matches!(user, "anon" | "none");
            let owned = user == "owner";
            let view = auth_runner::RunnerFacts {
                workspace: WorkspaceId::from("ws"),
                authenticated,
                visibility: auth_runner::VISIBILITY_PRIVATE,
                owned_by_requester: owned,
            };
            let key = |op: &str| format!("{user}x{op}_runner");
            let view_key = key("view");
            assert_eq!(
                auth_runner::can_view_runner(&view),
                matrix[view_key.as_str()].as_bool().expect("bool"),
                "{view_key}"
            );
            let use_key = key("use");
            assert_eq!(
                auth_runner::can_use_runner(&view),
                matrix[use_key.as_str()].as_bool().expect("bool"),
                "{use_key}"
            );
            // The machine matrix is the same rule on the same owner.
            let mkey = |op: &str| format!("{user}x{op}_machine");
            let mview_key = mkey("view");
            assert_eq!(
                auth_runner::can_view_runner(&view),
                matrix[mview_key.as_str()].as_bool().expect("bool"),
                "{mview_key}"
            );
            let muse_key = mkey("use");
            assert_eq!(
                auth_runner::can_use_runner(&view),
                matrix[muse_key.as_str()].as_bool().expect("bool"),
                "{muse_key}"
            );
            // `can_manage_runner(None)` short-circuits before touching
            // `.id` (fixture `nonexmanage_runner` is N/A + false).
            if user == "none" {
                assert_eq!(
                    matrix["nonexmanage_runner"].as_str(),
                    Some("N/A(none-has-no-.id)")
                );
                assert_eq!(matrix["none_manage_runner"].as_bool(), Some(false));
            }
            let manage = auth_runner::ManageFacts {
                workspace: WorkspaceId::from("ws"),
                requester: authenticated.then(|| UserId::from(user)),
                visibility: auth_runner::VISIBILITY_PRIVATE,
                owned_by_requester: owned,
                is_workspace_admin: user == "admin",
            };
            let manage_key = key("manage");
            let want_manage = if user == "none" {
                matrix["none_manage_runner"].as_bool().expect("bool")
            } else {
                matrix[manage_key.as_str()].as_bool().expect("bool")
            };
            assert_eq!(
                auth_runner::can_manage_runner(&scope, &manage),
                want_manage,
                "{manage_key}"
            );
        }
        // Non-private visibility (no such value exists; probed with 99):
        // every check denies, so the `is_workspace_admin` branch at
        // `permissions.py:137` is unreachable.
        for (user, admin) in [("owner", false), ("admin", true)] {
            let owned = user == "owner";
            let view = auth_runner::RunnerFacts {
                workspace: WorkspaceId::from("ws"),
                authenticated: true,
                visibility: 99,
                owned_by_requester: owned,
            };
            // The fixture pins the owner view row; the admin view row is
            // unpinned (deny-by-default holds for every caller).
            assert!(!auth_runner::can_view_runner(&view));
            if user == "owner" {
                assert_eq!(matrix["ownerxview_weird_runner"].as_bool(), Some(false));
            }
            let manage = auth_runner::ManageFacts {
                workspace: WorkspaceId::from("ws"),
                requester: Some(UserId::from(user)),
                visibility: 99,
                owned_by_requester: owned,
                is_workspace_admin: admin,
            };
            let weird_manage = format!("{user}xmanage_weird_runner");
            assert_eq!(
                auth_runner::can_manage_runner(&scope, &manage),
                matrix[weird_manage.as_str()].as_bool().expect("bool")
            );
        }
        assert_eq!(
            matrix["anon_manage_runner"].as_bool(),
            Some(auth_runner::can_manage_runner(
                &scope,
                &auth_runner::ManageFacts {
                    workspace: WorkspaceId::from("ws"),
                    requester: None,
                    visibility: auth_runner::VISIBILITY_PRIVATE,
                    owned_by_requester: false,
                    is_workspace_admin: false,
                }
            ))
        );
    }

    // -- predicate SQL ------------------------------------------------------

    /// The owner literal Django inlines unquoted in `str(qs.query)`.
    const OWNER_LITERAL: &str = "5cd0df65-d07d-496f-beff-db9750c1f375";

    #[test]
    fn runner_visible_predicate_matches_django() {
        let sql = &fx04()["runner_visible_to_user_q_sql"];
        let authed = runner_visible_predicate(OWNER_LITERAL, "runner");
        assert!(
            sql["authed"].as_str().expect("sql").contains(&authed),
            "authed WHERE carries {authed}"
        );
        // `prefix="runner__"` adds the join; the predicate text is the same
        // on the runner alias.
        let prefixed = sql["authed_prefix"].as_str().expect("sql");
        assert!(
            prefixed
                .contains(r#"INNER JOIN "runner" ON ("agent_run"."runner_id" = "runner"."id")"#),
            "prefix join"
        );
        assert!(
            prefixed.contains(&authed),
            "prefixed WHERE carries {authed}"
        );
        let anon = runner_visible_predicate_anon("runner");
        assert_eq!(anon, r#""runner"."id" IS NULL"#);
        assert!(
            sql["anon"].as_str().expect("sql").contains(&anon),
            "anon WHERE carries {anon}"
        );
        assert!(
            sql["none"].as_str().expect("sql").contains(&anon),
            "none WHERE carries {anon}"
        );
        // The Q shapes behind the SQL, for the record.
        assert!(sql["authed_q_repr"]
            .as_str()
            .expect("repr")
            .contains("Visibility.PRIVATE"));
        assert!(sql["anon_q_repr"]
            .as_str()
            .expect("repr")
            .contains("pk__isnull"));
    }

    #[test]
    fn runs_usable_predicate_matches_django() {
        let sql = &fx04()["filter_runs_usable_by_runner_sql"];
        let django = sql["private"].as_str().expect("sql");
        let ours = runs_usable_by_runner_predicate(0, OWNER_LITERAL, "agent_run").expect("private");
        // Our fragment is exactly the outer WHERE term: the `EXISTS`
        // subqueries nest their own WHEREs, so anchor on the FROM clause.
        let where_term = django
            .split_once(r#"FROM "agent_run" WHERE "#)
            .expect("outer WHERE")
            .1
            .strip_suffix(r#" ORDER BY "agent_run"."created_at" DESC"#)
            .expect("ORDER BY tail");
        assert_eq!(ours, where_term);
        // The through-model `deleted_at` guard binds to the same joined
        // row as the owner match.
        assert!(ours.contains(r#"(U2."assignee_id" = 5cd0df65-d07d-496f-beff-db9750c1f375 AND U2."deleted_at" IS NULL)"#));
        // Non-private runners match nothing (`qs.none()`).
        assert_eq!(
            runs_usable_by_runner_predicate(99, OWNER_LITERAL, "agent_run"),
            None
        );
        assert!(sql["non_private_is_none_qs"].as_bool().expect("flag"));
        assert!(sql["non_private_executes_to"]
            .as_array()
            .expect("rows")
            .is_empty());
    }

    // -- denial bodies ------------------------------------------------------

    #[test]
    fn denial_matches_fixture_bodies() {
        let denial = run_not_found();
        assert_eq!(denial.status, 404);
        assert_eq!(denial.body, json!({"error": "not found"}));
        let v = fx08();
        // Missing and forbidden share one body on every guard site.
        for (endpoint, keys) in [
            ("detail", ["missing", "forbidden"]),
            ("Cancel", ["missing", "forbidden"]),
        ] {
            let section = &v[endpoint.to_lowercase()];
            for key in keys {
                assert_eq!(
                    section[key]["status"].as_u64(),
                    Some(404),
                    "{endpoint}.{key}"
                );
                assert_eq!(section[key]["body"], denial.body, "{endpoint}.{key}");
            }
        }
        for key in ["missing", "forbidden"] {
            assert_eq!(v["release_pin"][key]["status"].as_u64(), Some(404));
            assert_eq!(v["release_pin"][key]["body"], denial.body);
        }
    }
}
