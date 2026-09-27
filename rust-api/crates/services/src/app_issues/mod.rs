#![forbid(unsafe_code)]

//! Project issue-list family (`app/views/issue/base.py`, pilot 2 of D-26).
//!
//! Ports the read slice of the hardest core-API endpoint family:
//! - `GET .../issues/` (`IssueViewSet.list`): grouped / sub-grouped / flat
//!   paginated list with `ComplexFilterBackend` + `IssueFilterSet` + legacy
//!   `issue_filters`, `updated_at__gt`, `order_by`, guest `created_by`
//!   scoping, and the expand/fields serializer path.
//! - `GET .../issues/list/?issues=` (`IssueListEndpoint.get`): flat fetch
//!   of an explicit id set, same filter stack.
//! - `GET .../v2/issues/` (`IssuePaginatedViewSet.list`): cursor page over
//!   `updated_at` with `description=true` opt-in.
//! - `GET .../deleted-issues/` (`DeletedIssuesListViewSet.get`):
//!   id list of archived-or-deleted issues.
//!
//! The SQL itself is assembled in `pidash-api` (`app_issues` handlers) on
//! the F-04 kernels (`pidash_db::{filter, filterset, issue_filters}`) and
//! the F-07 kernels (`pidash_api::{paginator, serializer}`). This module
//! owns everything above SQL text: query-param parsing with Django's exact
//! error bodies, the `order_issue_queryset` port, the response field lists,
//! the envelope assembly, and the timezone rendering rule.
//!
//! Ported bugs (also listed in the PR):
//! - `recent_visited_task.delay` fires inside the list action (and the flat
//!   `issues/list/` GET) but the suite environment never consumes it, so
//!   the Rust handlers perform no write. See [`RECENT_VISITED_NOTE`].
//! - `order_by=state__group` ascending maps to the *reversed* state order:
//!   `state_order = STATE_ORDER if order_by_param in ["state__name",
//!   "state__group"]` is always true on that branch, so the `[::-1]`
//!   alternative is dead. [`order_sql`] copies the dead branch.

pub mod ordering;
pub mod params;
pub mod shape;

pub use ordering::{order_sql, priority_case_sql, state_case_sql, OrderSpec};
pub use params::{ListParams, ParamError};
pub use shape::{
    deleted_ids_body, envelope, group_mismatch_body, issues_required_body, on_results_fields,
    v2_fields, DETAIL_FIELDS, FLAT_LIST_FIELDS, LIST_VALUES_FIELDS, ON_RESULTS_ARRAY_FIELDS,
    ON_RESULTS_BASE_FIELDS, ON_RESULTS_STATE_GROUP_FIELD, PRIORITY_VALUES, STATE_GROUP_VALUES,
    V2_REQUIRED_FIELDS,
};

/// What the Rust handlers do about the `recent_visited_task.delay` call
/// inside the Django list actions: nothing. `.delay()` is a deferred
/// publish and the suite environment never consumes it, so Django leaves
/// no observable trace; writing the row inline broke the gate's teardown
/// with rows Django never produces. Faithful deferral belongs to the tasks
/// layer. Response bytes and observable DB state match Django exactly.
pub const RECENT_VISITED_NOTE: &str = "recent_visited_task deferred to the tasks layer, no write";
