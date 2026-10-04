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
pub mod serializers_assoc;
pub mod serializers_detail;
pub mod serializers_engage;
pub mod serializers_label;
pub mod serializers_links;
pub mod serializers_refs;
pub mod shape;

pub use ordering::{order_sql, priority_case_sql, state_case_sql, OrderSpec};
pub use params::{parse_per_page, raw_group_mismatch, ListParams, ParamError, ParseOptions};
pub use serializers_detail::{
    agent_live_state_to_representation, agent_run_to_representation,
    agent_status_to_representation, agent_ticker_to_representation,
    app_issue_reaction_to_representation, app_issue_vote_to_representation,
    has_open_blockers_field, issue_detail_base_to_representation, issue_detail_to_representation,
    issue_lite_to_representation, issue_public_to_representation, relations_summary_field,
    serialize_drf_datetime, serialize_iso_datetime, AgentLiveStateRow, AgentLiveStateView,
    AgentRunDetailRow, AgentRunView, AgentStatusView, AgentTickerInput, AgentTickerView,
    AppIssueReactionRow, AppIssueReactionView, AppIssueVoteRow, AppIssueVoteView,
    IssueDetailBaseRow, IssueDetailBaseView, IssueDetailRow, IssueDetailView, IssueLiteRow,
    IssueLiteView, IssuePublicRow, IssuePublicView, ACTIVE_AGENT_RUN_SQL, ACTIVE_RUN_STATUSES,
    AGENT_LIVE_STATE_FIELDS, AGENT_RUN_COUNT_SQL, AGENT_RUN_FIELDS, AGENT_STATUS_FIELDS,
    AGENT_TICKER_FIELDS, APP_ISSUE_REACTION_FIELDS, APP_ISSUE_VOTE_FIELDS, ERROR_DIAGNOSTIC_FIELDS,
    ISSUE_DETAIL_BASE_FIELDS, ISSUE_DETAIL_FIELDS, ISSUE_DETAIL_RETRIEVE_FIELDS, ISSUE_LITE_FIELDS,
    ISSUE_PUBLIC_FIELDS, LATEST_AGENT_RUN_SQL,
};
pub use serializers_engage::{
    app_issue_flat_to_representation, comment_is_actively_synced, issue_activity_to_representation,
    issue_is_actively_synced, issue_reaction_lite_to_representation,
    issue_subscriber_to_representation, ActivitySourceData, AppIssueFlatRow, AppIssueFlatView,
    IssueActivityRow, IssueActivityView, IssueReactionLiteRow, IssueReactionLiteView,
    IssueSubscriberRow, IssueSubscriberView, APP_ISSUE_FLAT_FIELDS, GITHUB_COMMENT_SYNC_PROBE_SQL,
    GITHUB_ISSUE_SYNC_PROBE_SQL, GIT_COMMENT_SYNC_PROBE_SQL, GIT_ISSUE_SYNC_PROBE_SQL,
    ISSUE_ACTIVITY_ALL_FIELDS, ISSUE_REACTION_LITE_FIELDS, ISSUE_SUBSCRIBER_ALL_FIELDS,
};
pub use serializers_label::{
    app_label_to_representation, validate_label_name, AppLabelRow, AppLabelView, APP_LABEL_FIELDS,
    APP_LABEL_READONLY_INPUT_FIELDS, APP_LABEL_WRITABLE_FIELDS, LABEL_NAME_ALREADY_EXISTS,
    LABEL_NAME_CONFLICT_BODY, LABEL_NAME_CONFLICT_EXCLUDING_SELF_PROBE_SQL,
    LABEL_NAME_CONFLICT_PROBE_SQL,
};
pub use serializers_links::{
    git_code_review_link_to_representation, github_pull_request_link_to_representation,
    issue_attachment_lite_to_representation, issue_link_lite_to_representation,
    user_lite_to_representation, GitCodeReviewLinkRow, GitCodeReviewLinkView,
    GithubPullRequestLinkRow, GithubPullRequestLinkView, IssueAttachmentLiteRow,
    IssueAttachmentLiteView, IssueLinkLiteRow, IssueLinkLiteView, UserLiteRow, UserLiteView,
    GITHUB_PR_LINK_FIELDS, GIT_CODE_REVIEW_LINK_FIELDS, ISSUE_ATTACHMENT_LITE_FIELDS,
    ISSUE_LINK_LITE_FIELDS, USER_LITE_FIELDS,
};
pub use serializers_refs::{
    cycle_base_to_representation, issue_intake_to_representation, issue_state_to_representation,
    label_lite_to_representation, module_base_to_representation, project_lite_to_representation,
    state_lite_to_representation, CycleBaseRow, CycleBaseView, IssueIntakeRow, IssueIntakeView,
    IssueStateRow, IssueStateView, LabelLiteRow, LabelLiteView, ModuleBaseRow, ModuleBaseView,
    ProjectLiteRow, ProjectLiteView, StateLiteRow, StateLiteView, CYCLE_BASE_FIELDS,
    ISSUE_INTAKE_FIELDS, ISSUE_STATE_FIELDS, LABEL_LITE_FIELDS, MODULE_BASE_FIELDS,
    PROJECT_LITE_FIELDS, STATE_LITE_FIELDS,
};
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
