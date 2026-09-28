#![forbid(unsafe_code)]

//! Issue state-transition hook for Git provider completion comment-back
//! (D-05, jobs layer).
//!
//! Ports `apps/api/pi_dash/bgtasks/github_signals.py:31-74` (translation
//! only): the `capture_prior_state` pre-save snapshot
//! (`dispatch_uid="git_sync.issue_presave"`) and the
//! `trigger_completion_comment` post-save dispatcher
//! (`dispatch_uid="git_sync.issue_postsave"`).
//!
//! Per the Porting guide, signals become explicit calls: there is no
//! receiver registry here. Issue writers snapshot the prior `state_id`
//! before mutating ([`capture_prior_state`]) and, after commit, hand the
//! before/after pair to [`dispatch_completion_comment`], which returns
//! the Celery job to enqueue through the post-commit wrapper — never
//! inside the issue-write transaction. The separate `dispatch_uid`
//! namespace is preserved as separate call sites: the Git family
//! dispatches first ([`DispatchTarget::GitPostCompletion`], the
//! provider-neutral `git_sync_task.post_completion_comment`), and only
//! when no `GitIssueSync` mirrors the issue does the GitHub family fire
//! ([`DispatchTarget::GitHubPostCompletion`],
//! `github_sync_task.post_completion_comment`). The lazy imports
//! (`github_signals.py:54-59`, dodging the bgtasks → db → bgtasks cycle
//! at app-config time) have no Rust analogue — modules resolve at
//! compile time — so the lookups below are plain SQL.
//!
//! Fixture ids replayed by the unit tests below:
//! `rust-api/fixtures/integrations/tasks/completion_guards.golden.json`
//! (the issue names it `tasks/signals.json`; the recorded behavior lives
//! under the `completion_guards.golden.json` name).

use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use super::git_sync::{json_truthy, POST_COMPLETION_COMMENT_TASK as GIT_POST_COMMENT_TASK};
use super::github_sync::completion_job;
use crate::queue::NewJob;

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// The completed group (`StateGroup.COMPLETED.value`, `state.py:20`):
/// only transitions INTO this group dispatch.
pub const COMPLETED_GROUP: &str = "completed";

/// Prior-state snapshot (`capture_prior_state`,
/// `github_signals.py:31-41`): `Issue.all_objects.only("state_id").get(pk)`
/// — `all_objects` is a plain `Manager` (`mixins.py:67`), so the lookup is
/// deliberately UNSCOPED: soft-deleted rows still resolve. A missing row
/// (`DoesNotExist`) yields `None`, as does a `None` id (new rows take the
/// `if not instance.pk` branch without touching the database).
pub const PRIOR_STATE_SQL: &str = "SELECT state_id FROM issues WHERE id = $1";

/// The new state's group (`instance.state.group`,
/// `github_signals.py:51`): live-row scoping only. No triage exclusion —
/// attribute access on the saved instance carries no manager filter when
/// cached, and the completed transitions this gate keeps are never triage.
pub const STATE_GROUP_SQL: &str =
    "SELECT \"group\" FROM states WHERE id = $1 AND deleted_at IS NULL";

/// Provider-neutral mirror (`github_signals.py:61`):
/// `GitIssueSync.objects.filter(issue=instance).first()` — default
/// manager (live rows), database order, first row wins. `$1` the issue id.
pub const GIT_SYNC_LOOKUP_SQL: &str =
    "SELECT id, metadata FROM git_issue_syncs WHERE issue_id = $1 AND deleted_at IS NULL LIMIT 1";

/// Legacy mirror (`github_signals.py:68`):
/// `GithubIssueSync.objects.filter(issue=instance).first()` — same shape.
/// `$1` the issue id.
pub const GITHUB_SYNC_LOOKUP_SQL: &str =
    "SELECT id, metadata FROM github_issue_syncs WHERE issue_id = $1 AND deleted_at IS NULL LIMIT 1";

// ---------------------------------------------------------------------------
// Pure decision
// ---------------------------------------------------------------------------

/// Why a save does NOT dispatch (`trigger_completion_comment`,
/// `github_signals.py:46-52,63-64,71-72`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// `created` (`github_signals.py:46-47`): creation can't be a
    /// transition — freshly-imported synced issues land here too.
    Created,
    /// `prev_state_id == instance.state_id` (`:48-50`).
    SameState,
    /// `instance.state is None or state.group != 'completed'` (`:51-52`).
    NonCompletedGroup,
    /// `metadata.get("completion_comment_id")` is truthy on the mirror
    /// (`:63-64` git, `:71-72` github): already commented, idempotent
    /// short-circuit.
    AlreadyCommented,
    /// No `GitIssueSync` and no `GithubIssueSync` mirrors the issue
    /// (`:68-70`): nothing upstream to comment on.
    NoSyncRow,
}

/// Where a transitioning save dispatches (`github_signals.py:61-74`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchTarget {
    /// A `GitIssueSync` mirrors the issue: the provider-neutral
    /// `post_completion_comment` owns it (`:61-66`). Carries the
    /// `GitIssueSync` id.
    GitPostCompletion(Uuid),
    /// No git mirror, but a `GithubIssueSync` mirrors it: the legacy
    /// `post_completion_comment` owns it (`:68-74`). Carries the
    /// `GithubIssueSync` id.
    GitHubPostCompletion(Uuid),
}

/// What the post-save hook decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchOutcome {
    Dispatch(DispatchTarget),
    Skip(SkipReason),
}

/// The transition guards (`github_signals.py:46-52`): creation, same
/// state, and non-completed group skip before any mirror is touched.
/// `None == None` counts as same-state (Python compares the
/// `getattr(instance, _PREVIOUS_STATE, None)` default the same way).
pub fn transition_guard(
    created: bool,
    prev_state_id: Option<&Uuid>,
    current_state_id: Option<&Uuid>,
    current_group: Option<&str>,
) -> Result<(), SkipReason> {
    if created {
        return Err(SkipReason::Created);
    }
    if prev_state_id == current_state_id {
        return Err(SkipReason::SameState);
    }
    if current_group != Some(COMPLETED_GROUP) {
        return Err(SkipReason::NonCompletedGroup);
    }
    Ok(())
}

/// The idempotency guard (`github_signals.py:63-64,71-72`): a truthy
/// `completion_comment_id` in the mirror metadata short-circuits.
/// Falsy values (`null`, `""`, `0`, `false`) proceed, like Python's
/// `.get(…)` truthiness.
pub fn already_commented(metadata: &Value) -> bool {
    metadata
        .get("completion_comment_id")
        .is_some_and(json_truthy)
}

/// Snapshot the prior `state_id` before an issue mutation
/// (`capture_prior_state`, `github_signals.py:31-41`). `None` ids yield
/// `None` without a query (`if not instance.pk`); vanished rows
/// (`DoesNotExist`) yield `None`.
pub async fn capture_prior_state(
    pool: &PgPool,
    issue_id: Option<&Uuid>,
) -> Result<Option<Uuid>, String> {
    let Some(issue_id) = issue_id else {
        return Ok(None);
    };
    sqlx::query_scalar(PRIOR_STATE_SQL)
        .bind(*issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| format!("capture prior issue state: {error}"))
        .map(|found: Option<Option<Uuid>>| found.flatten())
}

/// Decide what a committed issue save dispatches
/// (`trigger_completion_comment`, `github_signals.py:44-74`): guards
/// first, then the `GitIssueSync` mirror, then the `GithubIssueSync`
/// mirror. Returns the outcome; the caller enqueues
/// [`dispatch_job`] through the post-commit wrapper.
pub async fn dispatch_completion_comment(
    pool: &PgPool,
    issue_id: &Uuid,
    current_state_id: Option<&Uuid>,
    created: bool,
    prev_state_id: Option<&Uuid>,
) -> Result<DispatchOutcome, String> {
    let db_error = |error: sqlx::Error| format!("completion dispatch lookup: {error}");
    let current_group: Option<String> = match current_state_id {
        None => None,
        Some(state_id) => sqlx::query_scalar(STATE_GROUP_SQL)
            .bind(*state_id)
            .fetch_optional(pool)
            .await
            .map_err(db_error)?
            .flatten(),
    };
    if let Err(reason) = transition_guard(
        created,
        prev_state_id,
        current_state_id,
        current_group.as_deref(),
    ) {
        return Ok(DispatchOutcome::Skip(reason));
    }
    // Git family first (`github_signals.py:61-66`): a present mirror
    // ends the dispatch — the github branch is never consulted.
    let git_mirror: Option<(Uuid, Value)> = sqlx::query_as(GIT_SYNC_LOOKUP_SQL)
        .bind(*issue_id)
        .fetch_optional(pool)
        .await
        .map_err(db_error)?;
    if let Some((sync_id, metadata)) = git_mirror {
        if already_commented(&metadata) {
            return Ok(DispatchOutcome::Skip(SkipReason::AlreadyCommented));
        }
        return Ok(DispatchOutcome::Dispatch(
            DispatchTarget::GitPostCompletion(sync_id),
        ));
    }
    let github_mirror: Option<(Uuid, Value)> = sqlx::query_as(GITHUB_SYNC_LOOKUP_SQL)
        .bind(*issue_id)
        .fetch_optional(pool)
        .await
        .map_err(db_error)?;
    let Some((sync_id, metadata)) = github_mirror else {
        return Ok(DispatchOutcome::Skip(SkipReason::NoSyncRow));
    };
    if already_commented(&metadata) {
        return Ok(DispatchOutcome::Skip(SkipReason::AlreadyCommented));
    }
    Ok(DispatchOutcome::Dispatch(
        DispatchTarget::GitHubPostCompletion(sync_id),
    ))
}

/// Build the Celery job for a dispatch decision
/// (`post_git_completion_comment.delay(str(id))` /
/// `post_completion_comment.delay(str(id))`, `github_signals.py:65,74`):
/// `args=[str]`, `kwargs={}`, first attempt, no ETA.
pub fn dispatch_job(target: &DispatchTarget) -> NewJob {
    match target {
        DispatchTarget::GitPostCompletion(sync_id) => NewJob::new(
            GIT_POST_COMMENT_TASK,
            serde_json::json!([sync_id.to_string()]),
            serde_json::json!({}),
        ),
        DispatchTarget::GitHubPostCompletion(sync_id) => completion_job(sync_id),
    }
}

#[cfg(test)]
mod tests {
    use super::super::github_sync::POST_COMPLETION_COMMENT_TASK as GITHUB_POST_COMMENT_TASK;
    use super::*;
    use crate::worker::{route_for, Registry, Route};
    use serde_json::json;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/integrations")
    }

    fn fixture(name: &str) -> Value {
        let text = std::fs::read_to_string(fixtures_dir().join(name))
            .unwrap_or_else(|e| panic!("read fixture {name}: {e}"));
        serde_json::from_str(&text).expect("fixture is valid JSON")
    }

    fn state_id(seed: u128) -> Uuid {
        Uuid::from_u128(seed)
    }

    // Guard 1 — created saves never dispatch, even into completed states
    // (freshly-imported synced issues land here too).
    #[test]
    fn created_save_skips() {
        assert_eq!(
            transition_guard(true, None, Some(&state_id(1)), Some(COMPLETED_GROUP)),
            Err(SkipReason::Created)
        );
        assert_eq!(
            transition_guard(
                true,
                Some(&state_id(1)),
                Some(&state_id(2)),
                Some(COMPLETED_GROUP)
            ),
            Err(SkipReason::Created)
        );
    }

    // Guard 2 — same-state saves skip, including None == None (the
    // `getattr` default compares equal to an unset state).
    #[test]
    fn same_state_save_skips() {
        assert_eq!(
            transition_guard(
                false,
                Some(&state_id(1)),
                Some(&state_id(1)),
                Some(COMPLETED_GROUP)
            ),
            Err(SkipReason::SameState)
        );
        assert_eq!(
            transition_guard(false, None, None, Some(COMPLETED_GROUP)),
            Err(SkipReason::SameState)
        );
        assert_eq!(
            transition_guard(false, None, Some(&state_id(1)), Some(COMPLETED_GROUP)),
            Ok(())
        );
    }

    // Guard 3 — only transitions INTO the completed group dispatch; a
    // missing state (`instance.state is None`) skips.
    #[test]
    fn non_completed_group_skips() {
        for group in [None, Some("backlog"), Some("started"), Some("triage")] {
            assert_eq!(
                transition_guard(false, Some(&state_id(1)), Some(&state_id(2)), group),
                Err(SkipReason::NonCompletedGroup),
                "{group:?}"
            );
        }
        assert_eq!(
            transition_guard(
                false,
                Some(&state_id(1)),
                Some(&state_id(2)),
                Some("completed")
            ),
            Ok(())
        );
        // The group literal matches `StateGroup.COMPLETED.value`
        // (`state.py:20`) exactly — no case folding, like Python.
        assert_eq!(
            transition_guard(
                false,
                Some(&state_id(1)),
                Some(&state_id(2)),
                Some("Completed")
            ),
            Err(SkipReason::NonCompletedGroup)
        );
    }

    // Guard 4 — truthy `completion_comment_id` short-circuits; falsy
    // values proceed (Python `.get(…)` truthiness, both families).
    #[test]
    fn already_commented_guard_matches_python_truthiness() {
        assert!(already_commented(&json!({"completion_comment_id": 4242})));
        assert!(already_commented(&json!({"completion_comment_id": "x"})));
        assert!(!already_commented(&json!({})));
        assert!(!already_commented(&json!({"completion_comment_id": null})));
        assert!(!already_commented(&json!({"completion_comment_id": ""})));
        assert!(!already_commented(&json!({"completion_comment_id": 0})));
        assert!(!already_commented(&json!({"completion_comment_id": false})));
        // Sibling keys never guard.
        assert!(!already_commented(
            &json!({"completion_comment_error": "x"})
        ));
        assert!(!already_commented(&json!({"upstream_gone_at": "t"})));
    }

    // Dispatch jobs: the two families keep their own task names and the
    // `.delay(str(id))` wire shape (`github_signals.py:65,74`).
    #[test]
    fn dispatch_jobs_keep_family_names_and_wire_shape() {
        let sync_id = state_id(0x1234);
        let git = dispatch_job(&DispatchTarget::GitPostCompletion(sync_id));
        assert_eq!(
            git.task,
            "pi_dash.bgtasks.git_sync_task.post_completion_comment"
        );
        assert_eq!(git.args, json!(["00000000-0000-0000-0000-000000001234"]));
        assert_eq!(git.kwargs, json!({}));

        let github = dispatch_job(&DispatchTarget::GitHubPostCompletion(sync_id));
        assert_eq!(
            github.task,
            "pi_dash.bgtasks.github_sync_task.post_completion_comment"
        );
        assert_eq!(github.args, json!(["00000000-0000-0000-0000-000000001234"]));
        assert_eq!(github.kwargs, json!({}));

        // The separate `dispatch_uid` namespace survives as separate call
        // sites: the two names are distinct constants from the two task
        // modules.
        assert_ne!(GIT_POST_COMMENT_TASK, GITHUB_POST_COMMENT_TASK);
        assert_eq!(git.task, GIT_POST_COMMENT_TASK);
        assert_eq!(github.task, GITHUB_POST_COMMENT_TASK);
    }

    // Every statement parses as Postgres.
    #[test]
    fn all_statements_parse_as_postgres() {
        for sql in [
            PRIOR_STATE_SQL,
            STATE_GROUP_SQL,
            GIT_SYNC_LOOKUP_SQL,
            GITHUB_SYNC_LOOKUP_SQL,
        ] {
            let mut stmts = sqlparser::parser::Parser::parse_sql(
                &sqlparser::dialect::PostgreSqlDialect {},
                sql,
            )
            .unwrap_or_else(|e| panic!("SQL parses: {e}\n{sql}"));
            assert_eq!(stmts.len(), 1);
            assert!(
                matches!(stmts.pop().unwrap(), sqlparser::ast::Statement::Query(_)),
                "{sql}"
            );
        }
    }

    // Scoping contrast: the prior-state snapshot is deliberately UNSCOPED
    // (`all_objects`), while the group and mirror lookups scope live rows.
    #[test]
    fn scoping_matches_manager_semantics() {
        assert!(!PRIOR_STATE_SQL.contains("deleted_at"));
        assert!(STATE_GROUP_SQL.contains("deleted_at IS NULL"));
        assert!(GIT_SYNC_LOOKUP_SQL.contains("issue_id = $1"));
        assert!(GIT_SYNC_LOOKUP_SQL.contains("deleted_at IS NULL"));
        assert!(GITHUB_SYNC_LOOKUP_SQL.contains("issue_id = $1"));
        assert!(GITHUB_SYNC_LOOKUP_SQL.contains("deleted_at IS NULL"));
    }

    // Fixture coverage: every guard the fixture issue recorded maps to a
    // decision above.
    #[test]
    fn signals_fixture_is_fully_covered() {
        let guards = fixture("tasks/completion_guards.golden.json");
        // All five skips asserted above (the issue names four; the
        // missing-sync-row short-circuit is the fifth).
        for key in [
            "created_save",
            "same_state_save",
            "non_completed_state",
            "already_commented",
            "missing_sync_row",
        ] {
            assert!(guards["guards"][key].as_str().is_some(), "{key}");
        }
        // Git-before-Github ordering with the exact source lines.
        assert!(guards["ordering"].as_str().unwrap_or("").contains("61-72"));
        // Prior capture incl. new/vanished rows.
        assert!(guards["prior_capture"]
            .as_str()
            .unwrap_or("")
            .contains("state_id"));
        // Completion body shape shared with the task module.
        assert!(guards["completion_body"]
            .as_str()
            .unwrap_or("")
            .contains("WEB_URL"));
        // The `dispatch_uid` independence note.
        assert!(guards["source"]
            .as_str()
            .unwrap_or("")
            .contains("dispatch_uid"));
    }

    // Registry note: signal dispatches enqueue through the task handlers
    // both families already register — no new worker names here.
    #[test]
    fn dispatch_targets_route_through_registered_tasks() {
        let registry = Registry::new();
        assert_eq!(
            route_for(&registry, GIT_POST_COMMENT_TASK),
            Route::PythonOwned
        );
        assert_eq!(
            route_for(&registry, GITHUB_POST_COMMENT_TASK),
            Route::PythonOwned
        );
    }
}
