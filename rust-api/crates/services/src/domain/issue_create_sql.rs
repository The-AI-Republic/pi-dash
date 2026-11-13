#![forbid(unsafe_code)]

//! Issue-create SQL (`Issue.save()` / `pidash_create_project_issue`).
//!
//! Moved from `dispatch::tools` in P3 step B (agent naming
//! convention): the only live survivors of the retired run tools.
//! The SDK re-exports point at this home.

use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Whether `ch` is stripped by `str.strip()` with no args:
/// `char::is_whitespace` plus U+001C–U+001F, which Python `str.isspace()`
/// accepts but Rust `char::is_whitespace` does not. U+0085 (NEL) is named
/// explicitly even though `is_whitespace` already covers it, so parity
/// with `str.strip()` does not hinge on that quirk.
fn is_py_space(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{85}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{1f}')
}

/// `value.strip()` (`tools.py:276,347`).
pub fn py_stripped(value: &str) -> &str {
    value.trim_matches(is_py_space)
}

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

/// SeaORM form of [`CREATE_SAVE_DEFAULT_STATE_SQL`] (`default = true`)
/// and [`CREATE_SAVE_FALLBACK_STATE_SQL`] (`default = false`): the first
/// live non-triage state of the project by `sequence`, projecting `id`.
pub fn create_save_state_select(
    project_id: Uuid,
    default: bool,
) -> sea_orm::sea_query::SelectStatement {
    use pidash_db::entity::states;
    use sea_orm::sea_query::{Expr, Order, Query};
    let col = |column: states::Column| Expr::col((states::Entity, column));
    let mut select = Query::select();
    select
        .column((states::Entity, states::Column::Id))
        .from(states::Entity)
        .and_where(col(states::Column::ProjectId).eq(project_id));
    if default {
        select.and_where(col(states::Column::Default).into());
    }
    select
        .and_where(col(states::Column::IsTriage).not())
        .and_where(col(states::Column::DeletedAt).is_null())
        .and_where(col(states::Column::Group).eq("triage").not())
        .order_by((states::Entity, states::Column::Sequence), Order::Asc)
        .limit(1)
        .to_owned()
}

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

/// SeaORM form of [`CREATE_SEQ_SCAN_SQL`]: the project's live
/// `MAX(sequence)`, output name `max_sequence`.
pub fn create_seq_scan_select(project_id: Uuid) -> sea_orm::sea_query::SelectStatement {
    use pidash_db::entity::issue_sequences;
    use sea_orm::sea_query::{Alias, Expr, Query};
    Query::select()
        .expr_as(
            Expr::col((issue_sequences::Entity, issue_sequences::Column::Sequence)).max(),
            Alias::new("max_sequence"),
        )
        .from(issue_sequences::Entity)
        .and_where(
            Expr::col((issue_sequences::Entity, issue_sequences::Column::ProjectId)).eq(project_id),
        )
        .and_where(
            Expr::col((issue_sequences::Entity, issue_sequences::Column::DeletedAt)).is_null(),
        )
        .to_owned()
}

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
