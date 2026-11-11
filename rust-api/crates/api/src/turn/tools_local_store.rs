//! Local tool data seam: the `ToolStore` trait plus the live `PgToolStore`
//! (PIDASHCONV-1113).
//!
//! The executors in [`super::tools_local`] own every decision — scoping
//! checks, validation, ordering, shaping, error mapping — and call this
//! seam for data. Method for method, each entry mirrors one ORM query (or
//! short query block) from `assistant/tools/*.py`, including the model
//! `save()` side effects (`Issue.save`'s sequence/housekeeping,
//! `IssueComment.save`'s `Description` sync) and the post-write fan-out
//! (`record_write` rows, `issue_activity` + sweep enqueues, the live
//! state-transition drive).
//!
//! [`PgToolStore`] is the SeaORM implementation, reusing the services
//! filter/FTS builders, the domain fault ports and the jobs
//! orchestration drivers. Unit tests drive the
//! executors through a fixture store (see `tools_local` tests); live-DB
//! proof of this file's SQL comes with PIDASHCONV-863's oracle replay,
//! the D-11 `tools_exec` precedent — except the fused update, whose
//! failure-injection live test lives in this file (PIDASHCONV-1124).

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_services::assistant::agent::AssistantDeps;
use pidash_services::domain::comments as comment_domain;
use pidash_services::domain::issues as issue_domain;
use pidash_services::domain::relations as relation_domain;

// ---------------------------------------------------------------------------
// Environment (injected settings + clock)
// ---------------------------------------------------------------------------

/// Tool inputs that are Django settings or clocks in Python, injected here
/// so library code never reads settings or the clock directly (the `ssrf`
/// injected-resolver precedent).
#[derive(Clone)]
pub struct ToolEnv {
    /// `settings.LOOP_PR_LOOKUPS_PER_RUN` (`github.py:95`).
    pub pr_lookups_cap: u32,
    /// Configured extra GitLab hosts (`GITLAB_HOST` /
    /// `GITLAB_ALLOWED_HOSTS`, already split).
    pub gitlab_hosts: Vec<String>,
    /// `settings.ASSISTANT_BLOCK_PRIVATE_URLS` (`ssrf.py:21-22`).
    pub block_private_urls: bool,
    /// DNS for the PR-URL SSRF guard (production: libc resolution;
    /// tests inject fakes).
    pub ssrf_resolver: Arc<dyn pidash_services::assistant::ssrf::HostResolver + Send + Sync>,
    /// Clock for row stamps and activity epochs.
    pub now: DateTime<Utc>,
}

impl Default for ToolEnv {
    fn default() -> Self {
        Self {
            pr_lookups_cap: pidash_services::assistant::tools_github::DEFAULT_PR_LOOKUPS_PER_RUN,
            gitlab_hosts: Vec::new(),
            block_private_urls: false,
            ssrf_resolver: Arc::new(pidash_services::assistant::ssrf::SystemResolver),
            now: Utc::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Store failures. `Missing` mirrors Django's `{Model} matching query
/// does not exist.` (`query.py:639-640`); `Db`/`Transition` carry the
/// backend message. All three map to a turn failure, never to the model:
/// in Python these exceptions escape the tool (they are not `ModelRetry`).
#[derive(Debug, Clone, PartialEq)]
pub enum StoreError {
    Db(String),
    Missing { model: &'static str },
    Transition(String),
}

impl StoreError {
    pub fn message(&self) -> String {
        match self {
            StoreError::Db(message) | StoreError::Transition(message) => message.clone(),
            StoreError::Missing { model } => format!("{model} matching query does not exist."),
        }
    }
}

impl From<sqlx::Error> for StoreError {
    fn from(error: sqlx::Error) -> Self {
        StoreError::Db(error.to_string())
    }
}

// ---------------------------------------------------------------------------
// Row types (one per query shape the tools read)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectRow {
    pub id: Uuid,
    pub identifier: String,
    pub name: String,
    pub workspace_id: Uuid,
    pub guest_view_all_features: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IssueRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub project_identifier: String,
    pub workspace_id: Uuid,
    pub sequence_id: i64,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub state_id: Option<Uuid>,
    pub state_name: Option<String>,
    pub state_group: Option<String>,
    pub priority: String,
    pub description_stripped: Option<String>,
    pub created_by_id: Option<Uuid>,
    /// Dedicated target (PDASHOSS01-350): the pod / runner the issue
    /// routes to (`None` = pod default / any runner in the pod).
    pub assigned_pod_id: Option<Uuid>,
    pub assigned_runner_id: Option<Uuid>,
    /// Read for the `-updated_at` page order (never rendered).
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IssueCommentRow {
    pub comment_stripped: Option<String>,
    pub created_at: DateTime<Utc>,
    pub speaker_type: String,
    pub actor_id: Option<Uuid>,
    pub actor_display_name: Option<String>,
    pub actor_email: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StateRow {
    pub id: Uuid,
    pub name: String,
    /// Never null (`state.py:99-103`, default `backlog`).
    pub group: String,
    pub default: bool,
    pub sequence: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LabelRow {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemberRow {
    pub member_id: Uuid,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub role: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentRunRow {
    pub id: Uuid,
    pub status: String,
    pub created_at: Option<DateTime<Utc>>,
    /// The run's pin (PDASHOSS01-350): the dedicated runner and kind.
    pub pinned_runner_id: Option<Uuid>,
    pub pinned_runner_name: Option<String>,
    pub pin_kind: String,
    /// The waiting object while the run waits for its runner (null
    /// otherwise), shaped by the store from the domain service.
    pub dispatch_wait: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PairRow {
    pub id: Uuid,
    pub issue_id: Uuid,
    pub related_issue_id: Uuid,
    pub relation_type: String,
}

/// The other end of a grouped relation (`relations._item` inputs).
#[derive(Debug, Clone, PartialEq)]
pub struct GroupedIssueRow {
    pub id: Uuid,
    pub project_identifier: String,
    pub sequence_id: i64,
    pub name: String,
    pub state_name: Option<String>,
    pub state_group: Option<String>,
}

/// `issue_activity.delay` enqueue the relation tools fan out
/// (`relations._log_activity`).
#[derive(Debug, Clone, PartialEq)]
pub struct RelationActivity {
    pub activity_type: String,
    /// `requested` renders (`relate`: the batch; `unrelate`: per target).
    pub requested_data: Option<String>,
    pub actor_id: Uuid,
    pub issue_id: Uuid,
    pub project_id: Uuid,
    pub current_instance: Option<String>,
    pub epoch: i64,
}

/// What `handle_issue_state_transition` decided (`service.py:69-73`).
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionOutcome {
    pub created_run_id: Option<Uuid>,
    pub reason: String,
}

/// The `get_issue` execution detail (PDASHOSS01-350).
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionDetail {
    /// The dedicated runner detail (null when undedicated).
    pub runner_detail: Value,
    /// The latest run's waiting object (null when nothing waits).
    pub dispatch_wait: Value,
}

/// Inputs to the issue create (`issues.create_issue :265-277` plus the
/// `Issue.save` housekeeping the store applies). `assigned_pod_id` /
/// `assigned_runner_id` carry the dedicated target (PDASHOSS01-350);
/// the executor validates them through the shared service first and
/// applies the runner after the create, as the API serializers do.
#[derive(Debug, Clone, PartialEq)]
pub struct NewIssueInput {
    pub project_id: Uuid,
    pub project_identifier: String,
    pub workspace_id: Uuid,
    pub name: String,
    pub description_html: String,
    pub priority: String,
    pub state_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_via: String,
    pub assigned_pod_id: Option<Uuid>,
    pub assigned_runner_id: Option<Uuid>,
}

/// Field patch for the issue update (`issues.update_issue :337-361`).
/// `state_given` is set only when the state arm runs (`if to_state is
/// not None`, `issues.py:351`): an explicit empty `state_id` that
/// resolves to no state (stateless project) skips the arm entirely —
/// no write, no `changed` entry.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IssuePatch {
    pub name: Option<String>,
    pub description_html: Option<String>,
    pub priority: Option<String>,
    pub state_given: bool,
    pub state_id: Option<Uuid>,
    pub parent_touched: bool,
    pub parent_id: Option<Uuid>,
    /// Pod write (PDASHOSS01-350): set to the requested pod, or `None`
    /// to clear when `pod_touched`.
    pub pod_touched: bool,
    pub pod_id: Option<Uuid>,
    /// Dedicated-runner apply (PDASHOSS01-350): when set, the fused
    /// transaction stamps the choice after the save and before the
    /// dispatch (the Django order), through the shared service.
    pub apply_runner: bool,
    pub new_runner_id: Option<Uuid>,
}

/// Inputs to the comment create (`comments.create_comment :53-65` plus
/// the `IssueComment.save` `Description` sync).
#[derive(Debug, Clone, PartialEq)]
pub struct NewCommentInput {
    pub issue_id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub html: String,
    pub fold: bool,
}

/// PR fetch outcome (`github.py:125-145`): fetched (status + optional
/// parsed body; `None` is undecodable JSON → `bad_response`) or a
/// transport failure (any exception → `network_error`).
#[derive(Debug, Clone, PartialEq)]
pub enum PrFetch {
    Fetched { status: u16, body: Option<Value> },
    NetworkError,
}

// ---------------------------------------------------------------------------
// The seam
// ---------------------------------------------------------------------------

/// Data access for the local tool executors. Every method mirrors one
/// ORM query block; scoping predicates live inside the SQL exactly as
/// the querysets compose them. Ordering and limits follow the tool's
/// queryset (`order_by` / slices); `project_states` is the one unordered
/// read, mirroring `_scoping.project_states` (the executors select from
/// it as `_resolve_state` / the ticking default-target lookup do).
#[allow(async_fn_in_trait)]
pub trait ToolStore {
    /// `User.objects.get(pk)` (`_scoping.user_for`); missing is
    /// `StoreError::Missing`.
    async fn require_user(&self, deps: &AssistantDeps) -> Result<Uuid, StoreError>;

    /// `member_projects(deps)` in name order, first 50
    /// (`projects.list_projects :20`).
    async fn member_projects(&self, deps: &AssistantDeps) -> Result<Vec<ProjectRow>, StoreError>;

    /// `member_projects(deps).filter(id)` (`_scoping.get_project`).
    async fn member_project(
        &self,
        deps: &AssistantDeps,
        project_id: &Uuid,
    ) -> Result<Option<ProjectRow>, StoreError>;

    /// `scoped_issues(deps).filter(id)` with project + state
    /// (`_scoping.get_issue`).
    async fn scoped_issue(
        &self,
        deps: &AssistantDeps,
        issue_id: &Uuid,
    ) -> Result<Option<IssueRow>, StoreError>;

    /// `scoped_issues` page for `search_issues`: optional project
    /// narrowing, FTS when the query is non-blank
    /// (`search.issue.issue_search_queryset`), `-updated_at` order, the
    /// `offset..offset+limit+1` window.
    async fn search_issues_page(
        &self,
        deps: &AssistantDeps,
        project_id: Option<Uuid>,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<IssueRow>, StoreError>;

    /// `scoped_issues` page for `list_issues`: project filter, the
    /// compiled `work_item_list_filters` predicates, `-updated_at`
    /// order, the lookahead window.
    async fn list_issues_page(
        &self,
        deps: &AssistantDeps,
        project_id: &Uuid,
        filters: &pidash_services::assistant::tools_issues::ListFilterPredicates,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<IssueRow>, StoreError>;

    /// `my_issues(deps, scope)` page (`core.querysets`), `-updated_at`
    /// order, the lookahead window.
    async fn my_issues_page(
        &self,
        deps: &AssistantDeps,
        scope: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<IssueRow>, StoreError>;

    /// `issue.issue_comments...order_by("-created_at")[:10]` with actors
    /// (`issues.get_issue :161`).
    async fn issue_comments(&self, issue_id: &Uuid) -> Result<Vec<IssueCommentRow>, StoreError>;

    /// `State.objects.filter(project_id, workspace__slug)`
    /// (`_scoping.project_states`), unordered, triage- and
    /// soft-deleted-excluded via the `State` default manager.
    async fn project_states(
        &self,
        deps: &AssistantDeps,
        project_id: &Uuid,
    ) -> Result<Vec<StateRow>, StoreError>;

    /// `Label.objects.filter(project_id, workspace__slug)` in name
    /// order (`projects.list_labels :41-43`).
    async fn project_labels(
        &self,
        deps: &AssistantDeps,
        project_id: &Uuid,
    ) -> Result<Vec<LabelRow>, StoreError>;

    /// Workspace-level labels (no project) for `_resolve_labels`
    /// (`issue_filters.py:531`).
    async fn workspace_labels(&self, deps: &AssistantDeps) -> Result<Vec<LabelRow>, StoreError>;

    /// `Issue.issue_objects` by identifier in the workspace
    /// (`_resolve_parents`): triage/archived/draft excluded, membership
    /// NOT consulted.
    async fn workspace_issue_by_identifier(
        &self,
        deps: &AssistantDeps,
        project_ident: &str,
        sequence_id: i64,
    ) -> Result<Option<IssueRow>, StoreError>;

    /// Active project members with users (`projects.list_project_members
    /// :51-54`), skipping null-member rows.
    async fn project_members(
        &self,
        deps: &AssistantDeps,
        project_id: &Uuid,
    ) -> Result<Vec<MemberRow>, StoreError>;

    /// `check_project_role(..., [ADMIN, MEMBER])`
    /// (`_scoping.require_project_write`).
    async fn check_project_write(
        &self,
        deps: &AssistantDeps,
        project_id: &Uuid,
    ) -> Result<bool, StoreError>;

    /// `AgentRun.objects.filter(work_item_id).order_by("-created_at")[:5]`
    /// (`runs.get_run_status :24`).
    async fn agent_runs(&self, issue_id: &Uuid) -> Result<Vec<AgentRunRow>, StoreError>;

    /// Runners of a project visible to the acting user for name
    /// resolution (PDASHOSS01-350): `(id, name)` newest-first.
    async fn list_runners_for_resolution(
        &self,
        project_id: &Uuid,
        actor_id: &Uuid,
    ) -> Result<Vec<(Uuid, String)>, StoreError>;

    /// Pods of a project for name resolution (PDASHOSS01-350):
    /// `(id, name)` in name order.
    async fn list_pods_for_resolution(
        &self,
        project_id: &Uuid,
    ) -> Result<Vec<(Uuid, String)>, StoreError>;

    /// A runner by id within a project (`None` when it does not exist
    /// or belongs to another project). Returns the runner id and its
    /// pod (the pod follows the runner, §7.2).
    async fn runner_by_id_in_project(
        &self,
        project_id: &Uuid,
        runner_id: &Uuid,
    ) -> Result<Option<(Uuid, Uuid)>, StoreError>;

    /// A pod by id within a project (`None` otherwise).
    async fn pod_by_id_in_project(
        &self,
        project_id: &Uuid,
        pod_id: &Uuid,
    ) -> Result<Option<Uuid>, StoreError>;

    /// The shared E-code validation (PDASHOSS01-350): an `E-code`
    /// refusal surfaces as `StoreError::Transition` carrying the
    /// tool-channel message (detail + machine code).
    async fn validate_execution_target_write(
        &self,
        write: &pidash_services::domain::issue_assignment::ExecutionTargetWrite,
    ) -> Result<(), StoreError>;

    /// The post-commit apply (PDASHOSS01-350): stamps the runner (+ pod),
    /// re-points a queued run, and writes the activity row in its own
    /// transaction (the create-path order; the update path fuses this
    /// into `update_issue_row`).
    async fn apply_execution_target_post_commit(
        &self,
        issue_id: &Uuid,
        new_runner_id: Option<Uuid>,
        actor_id: &Uuid,
    ) -> Result<(), StoreError>;

    /// Whether the issue has a blocking run (the pod-change guard,
    /// PDASHOSS01-350).
    async fn has_blocking_run(&self, issue_id: &Uuid) -> Result<bool, StoreError>;

    /// The `get_issue` execution detail (PDASHOSS01-350): the dedicated
    /// runner detail and the latest run's waiting object.
    async fn execution_detail(
        &self,
        issue_id: &Uuid,
        actor_id: &Uuid,
    ) -> Result<ExecutionDetail, StoreError>;

    /// The `list_project_runners` read (PDASHOSS01-350): the visible
    /// runners of a project, most-recent-first, with the optional
    /// assignability row fields.
    async fn list_project_runners(
        &self,
        project_id: &Uuid,
        actor_id: &Uuid,
        assignable_for: Option<&str>,
    ) -> Result<Vec<Value>, StoreError>;

    /// `scoped_issues.filter(project__identifier__iexact,
    /// sequence_id)` — the identifier arm of `resolve_refs`.
    async fn scoped_issue_by_identifier(
        &self,
        deps: &AssistantDeps,
        project_ident: &str,
        sequence_id: i64,
    ) -> Result<Option<IssueRow>, StoreError>;

    /// Live relation rows between two issues (`relations._pair_rows`).
    async fn pair_rows(&self, a_id: &Uuid, b_id: &Uuid) -> Result<Vec<PairRow>, StoreError>;

    /// Insert one relation row (`relations.relate :196-207`); `Ok(false)`
    /// is the unique-violation race (re-read, don't fail).
    #[allow(clippy::too_many_arguments)]
    async fn insert_relation(
        &self,
        issue_id: &Uuid,
        related_id: &Uuid,
        stored_type: &str,
        project_id: &Uuid,
        workspace_id: &Uuid,
        actor_id: &Uuid,
        now: &DateTime<Utc>,
    ) -> Result<bool, StoreError>;

    /// Soft-delete one relation row + its sweep enqueue
    /// (`relations.unrelate` + `SoftDeleteModel.delete` fan-out).
    async fn delete_relation(
        &self,
        row_id: &Uuid,
        actor_id: &Uuid,
        now: &DateTime<Utc>,
    ) -> Result<(), StoreError>;

    /// Best-effort `issue_activity` fan-out (`relations._log_activity`).
    async fn enqueue_relation_activity(
        &self,
        activity: &RelationActivity,
    ) -> Result<(), StoreError>;

    /// Every live row touching the issue in its workspace, self-pairs
    /// excluded (`relations.grouped_relations` row fetch).
    async fn grouped_relation_rows(
        &self,
        issue_id: &Uuid,
        workspace_id: &Uuid,
    ) -> Result<Vec<PairRow>, StoreError>;

    /// The other ends narrowed to the viewer's visible pool
    /// (`grouped_relations` second fetch, `id__in` + workspace).
    async fn visible_grouped_issues(
        &self,
        deps: &AssistantDeps,
        ids: &[Uuid],
        workspace_id: &Uuid,
    ) -> Result<Vec<GroupedIssueRow>, StoreError>;

    /// One ancestor-chain step for the `_resolve_parent` cycle walk
    /// (`Issue.objects.filter(pk).values_list("parent_id").first()` —
    /// note the unscoped manager).
    async fn issue_parent_id(&self, issue_id: &Uuid) -> Result<Option<Uuid>, StoreError>;

    /// Full create: the tool's project lock, the `Issue.save`
    /// advisory lock + sequence allocation + housekeeping, the issue
    /// INSERT, the `IssueSequence` row — one transaction.
    async fn create_issue_row(
        &self,
        input: &NewIssueInput,
        now: &DateTime<Utc>,
    ) -> Result<IssueRow, StoreError>;

    /// Row-locked patch plus fused transition drive: `SELECT FOR
    /// UPDATE`, the `save(update_fields)` column write (exactly the
    /// tool's fields — `description_stripped` stays stale, as in
    /// Python), and — when `to_state` is given and differs from the
    /// locked row's pre-save state — `handle_issue_state_transition`
    /// on the SAME transaction, one commit (`issues.update_issue
    /// :334-381`). Returns the saved row plus the drive outcome
    /// (`None` when no drive ran). A mid-drive failure rolls the edit
    /// back, as in Python (PIDASHCONV-1124).
    async fn update_issue_row(
        &self,
        patch: &IssuePatch,
        issue_id: &Uuid,
        to_state: Option<pidash_services::orchestration::creation::StateView>,
        actor_id: &Uuid,
        now: &DateTime<Utc>,
    ) -> Result<(IssueRow, Option<TransitionOutcome>), StoreError>;

    /// `handle_issue_state_transition(..., dispatch_immediate=True)`
    /// with an immediate drain, exactly as the tools call it (actor, no
    /// `moved_by_run`). Serves `dispatch_coding_run` only since
    /// PIDASHCONV-1124 (the update path fuses its drive into
    /// `update_issue_row`). `created_by` is the crum user for the
    /// ticker create-shape: `Some(actor)` on the `update_issue` path
    /// (which runs impersonated), `None` on the `dispatch_coding_run`
    /// path. A finalized run is a normal outcome, not an error: the
    /// prompt-build failure path commits the FAILED run with no
    /// exception, so the drive commits and drains its publish pair.
    #[allow(clippy::too_many_arguments)]
    async fn drive_transition(
        &self,
        issue_id: &Uuid,
        from_state: Option<pidash_services::orchestration::creation::StateView>,
        to_state: pidash_services::orchestration::creation::StateView,
        actor_id: &Uuid,
        created_by: Option<Uuid>,
        now: &DateTime<Utc>,
        pod_once: bool,
    ) -> Result<TransitionOutcome, StoreError>;

    /// The `create_issue` post-save fire (`signals.py:74-108`): a
    /// post-commit `handle_issue_state_transition(from=None, to=state,
    /// actor=None, dispatch_immediate=True)` on its own transaction,
    /// reached through the services `fire_state_transition` receiver
    /// (PIDASHCONV-1251). Preflight runs on the drive transaction —
    /// the `update_issue_row` probe/stub/re-run shape — so the bounce
    /// sees the drive's own uncommitted ticker row (a pool-side bounce
    /// would see no row and skip the explicit disarm).
    ///
    /// The tool saves through `Issue.save` (`issues.py:256-277`), so
    /// the receiver fires with prior state None and the saved state;
    /// `actor=None` is the signal default (`fire_transition_request`),
    /// and a handler failure is swallowed with the verbatim log line
    /// and a bumped counter — the save stands. `created_by` is the
    /// actor: the create path runs impersonated (`issues.py:256`,
    /// `with impersonate(user)`), as the update path does. Returns the
    /// drive outcome (`None` when the handler was skipped or its
    /// failure swallowed). A state-*lookup* storage failure propagates,
    /// exactly the `try` placement in `fire_state_transition` (the
    /// `fire_after_save` precedent), as does a post-commit dispatch
    /// failure — Django dispatches from `on_commit`, outside the
    /// signal `try`, so its tool fails there too (PIDASHCONV-1571).
    /// A finalized run is a normal outcome, not an error: the
    /// prompt-build failure path commits the FAILED run with no
    /// exception, so the drive commits and drains its publish pair.
    async fn drive_create_transition(
        &self,
        issue_id: &Uuid,
        to_state: pidash_services::orchestration::creation::StateView,
        actor_id: &Uuid,
        now: &DateTime<Utc>,
    ) -> Result<Option<TransitionOutcome>, StoreError>;

    /// Comment INSERT + `Description` INSERT + `description_id` link, one
    /// transaction (`IssueComment.save`).
    async fn create_comment_row(
        &self,
        input: &NewCommentInput,
        now: &DateTime<Utc>,
    ) -> Result<Uuid, StoreError>;

    /// `record_write`: the `TOOL_RESULT` message row (own transaction)
    /// plus the `tool_result` event row (own transaction) and the
    /// best-effort Redis publish, swallowed on any failure.
    async fn record_write(
        &self,
        deps: &AssistantDeps,
        summary: &str,
        links: &[Value],
        now: &DateTime<Utc>,
    ) -> Result<(), StoreError>;

    /// Newest binding credential config on a member project matching
    /// provider + namespace/name (`github._find_token` first arm).
    async fn github_binding_config(
        &self,
        deps: &AssistantDeps,
        provider: &str,
        namespace: &str,
        repo: &str,
    ) -> Result<Option<Value>, StoreError>;

    /// Newest GitHub sync credentials on a member project
    /// (`github._find_token` fallback arm).
    async fn github_sync_credentials(
        &self,
        deps: &AssistantDeps,
        namespace: &str,
        repo: &str,
    ) -> Result<Option<Value>, StoreError>;

    /// Decrypt a stored token (`license.utils.encryption.decrypt_data`
    /// — catches everything, returns `""` on failure).
    fn decrypt_token(&self, stored: Option<&str>) -> Option<String>;

    /// One PR/MR API fetch: 10s timeout, no redirects
    /// (`github.py:125-127`).
    async fn fetch_pr(&self, api_url: &str, headers: &[(String, String)]) -> PrFetch;
}

/// Shared redis handle type for the assistant publish (the bin's
/// multiplexed client, as the runner seam providers use it).
pub type RedisClient = Arc<redis::Client>;

// ---------------------------------------------------------------------------
// Live store
// ---------------------------------------------------------------------------

use pidash_db::entity::{
    agent_run, assistant_event, assistant_message, assistant_thread, assistant_turn,
    assistant_user_llm_config, git_provider_accounts, git_repositories, git_repository_bindings,
    github_repositories, github_repository_syncs, issue_comments, issue_relations, issue_sequences,
    issues, labels, pod, project_members, projects, runner, states, users, workspaces,
};
use pidash_services::assistant::tools_issues::ListFilterPredicates;
use sea_orm::sea_query::{Alias, Expr, Func};
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DbErr, EntityTrait, FromQueryResult, IntoSimpleExpr,
    JoinType, QueryFilter, QueryOrder, QuerySelect, QueryTrait, RelationTrait,
};

/// The live [`ToolStore`]: sqlx over the worker pool, reqwest for the PR
/// fetch, the multiplexed redis client for the swallowed publish, and
/// the D-11/D-12 delegation inputs for the transition drive.
#[derive(Clone)]
pub struct PgToolStore {
    pool: PgPool,
    redis: Option<RedisClient>,
    http: reqwest::Client,
    keyring: pidash_db::config::encryption::Keyring,
    cloud: pidash_db::config::CloudAgentSettings,
    managed: pidash_db::config::ManagedRunnerSettings,
    toolsets: Arc<dyn pidash_services::extensions::CloudAgentToolsetsSeam>,
}

impl PgToolStore {
    /// Fused-drive probe savepoint (`update_issue_row` /
    /// `drive_create_transition`): the probe's bounce writes roll back.
    /// `~Transaction` has no nested-begin, so a named savepoint is the
    /// reviewed escape hatch here (same probe shape on both drives).
    async fn probe_savepoint(
        &self,
        tx: &mut pidash_db::tx::Transaction<'_>,
    ) -> Result<(), StoreError> {
        // A named SAVEPOINT is the preview-rollback shape the
        // raw-sql: named SAVEPOINT is the preview-rollback shape the fused drives keep; no nested-begin escape on Transaction.
        sqlx::query("SAVEPOINT turn_preflight_probe")
            .execute(&mut **tx.inner())
            .await?;
        Ok(())
    }

    async fn rollback_probe_savepoint(
        &self,
        tx: &mut pidash_db::tx::Transaction<'_>,
    ) -> Result<(), StoreError> {
        // raw-sql: named-savepoint rollback for the fused-drive probe.
        sqlx::query("ROLLBACK TO SAVEPOINT turn_preflight_probe")
            .execute(&mut **tx.inner())
            .await?;
        Ok(())
    }

    /// `projects.list_projects :20`: member projects in the slug's
    /// workspace, name order, first 50 (`DISTINCT` against re-added
    /// memberships, `_scoping.py:45-49`).
    fn member_projects_query(deps: &AssistantDeps) -> sea_orm::Select<projects::Entity> {
        projects::Entity::find()
            .join(
                JoinType::InnerJoin,
                projects::Relation::ProjectMembers.def(),
            )
            .join(JoinType::InnerJoin, projects::Relation::Workspaces.def())
            .filter(project_members::Column::MemberId.eq(deps.user_id))
            .filter(project_members::Column::IsActive.eq(true))
            .filter(workspaces::Column::Slug.eq(&deps.workspace_slug))
            .filter(projects::Column::DeletedAt.is_null())
            .distinct()
            .order_by_asc(projects::Column::Name)
            .limit(50)
    }

    /// `tools_github::BINDING_TOKEN_SQL`: newest binding on a member
    /// project whose repository matches provider + namespace/name
    /// case-insensitively; the `credential_config` value rides through.
    fn binding_config_query(
        deps: &AssistantDeps,
        provider: &str,
        namespace: &str,
        repo: &str,
    ) -> sea_orm::Select<git_repository_bindings::Entity> {
        let member_projects = projects::Entity::find()
            .join(
                JoinType::InnerJoin,
                projects::Relation::ProjectMembers.def(),
            )
            .join(JoinType::InnerJoin, projects::Relation::Workspaces.def())
            .select_only()
            .column(projects::Column::Id)
            .distinct()
            .filter(project_members::Column::MemberId.eq(deps.user_id))
            .filter(project_members::Column::IsActive.eq(true))
            .filter(workspaces::Column::Slug.eq(&deps.workspace_slug))
            .filter(projects::Column::DeletedAt.is_null())
            .into_query();
        git_repository_bindings::Entity::find()
            .join(
                JoinType::InnerJoin,
                git_repository_bindings::Relation::GitRepositories.def(),
            )
            .join(
                JoinType::InnerJoin,
                git_repository_bindings::Relation::GitProviderAccounts.def(),
            )
            .filter(git_repository_bindings::Column::ProjectId.in_subquery(member_projects))
            .filter(git_repositories::Column::Provider.eq(provider))
            .filter(
                Expr::expr(Func::upper(Expr::col((
                    git_repositories::Entity,
                    git_repositories::Column::Namespace,
                ))))
                .eq(Func::upper(Expr::val(namespace.to_owned()))),
            )
            .filter(
                Expr::expr(Func::upper(Expr::col((
                    git_repositories::Entity,
                    git_repositories::Column::Name,
                ))))
                .eq(Func::upper(Expr::val(repo.to_owned()))),
            )
            .filter(git_repository_bindings::Column::DeletedAt.is_null())
            .order_by_desc(git_repository_bindings::Column::CreatedAt)
    }

    pub fn new(
        pool: PgPool,
        redis: Option<RedisClient>,
        cloud: pidash_db::config::CloudAgentSettings,
        managed: pidash_db::config::ManagedRunnerSettings,
    ) -> Result<Self, StoreError> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(
                pidash_services::assistant::tools_github::PR_LOOKUP_TIMEOUT_S,
            ))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| StoreError::Db(error.to_string()))?;
        Ok(Self {
            pool,
            redis,
            http,
            keyring: pidash_db::config::encryption::Keyring::from_env(),
            cloud,
            managed,
            toolsets: Arc::new(pidash_services::extensions::NoExtraToolsets),
        })
    }

    /// Post-commit completion comment-back for a state-changing save
    /// (`github_signals.py:44-75`, via the jobs-layer port). The caller
    /// gates on its own transition check; this resolves the dispatch
    /// (group, mirror, idempotency guards) and enqueues the provider's
    /// `post_completion_comment` job. Best-effort: failures only warn.
    async fn enqueue_completion_comment(
        &self,
        issue_id: &Uuid,
        prev_state_id: Option<&Uuid>,
        current_state_id: Option<&Uuid>,
    ) {
        use pidash_jobs::integrations::github_signals::{
            dispatch_completion_comment, dispatch_job, DispatchOutcome,
        };
        let outcome = dispatch_completion_comment(
            &self.pool,
            issue_id,
            current_state_id,
            false,
            prev_state_id,
        )
        .await;
        let target = match outcome {
            Ok(DispatchOutcome::Dispatch(target)) => target,
            Ok(DispatchOutcome::Skip(_)) => return,
            Err(error) => {
                tracing::warn!(%error, "completion comment dispatch lookup failed; op stands");
                return;
            }
        };
        let job = dispatch_job(&target);
        if let Err(error) = pidash_jobs::queue::enqueue(&self.pool, &job).await {
            tracing::warn!(%error, task = %job.task, "task enqueue failed; op stands");
        }
    }
}

/// `Issue.issue_objects` conjuncts past `deleted_at` (the scoped-issues
/// WHERE over the `states` LEFT JOIN + `projects` INNER JOIN):
/// `NOT (states.group = 'triage' AND states.group IS NOT NULL)` keeps
/// stateless issues (the inner `AND` is FALSE, not NULL, for them) plus
/// the archived/draft exclusions. Same null-safe triage trap as
/// `tools_issues::SCOPED_ISSUES_SQL` / `handlers_core`'s
/// `issue_objects_condition`.
fn issue_objects_condition() -> Condition {
    Condition::all()
        .add(
            Condition::all()
                .add(states::Column::Group.eq("triage"))
                .add(states::Column::Group.is_not_null())
                .not(),
        )
        .add(issues::Column::ArchivedAt.is_null())
        .add(projects::Column::ArchivedAt.is_null())
        .add(issues::Column::IsDraft.eq(false))
}

/// `my_issues_scope_sql` (`user_issues_queryset`, `core/querysets.py:33-52`)
/// as SeaORM: `assigned` / `created` / `all` (assigned OR created OR
/// subscribed). The soft-delete asymmetry is preserved: the M2M `EXISTS`
/// arms carry NO `deleted_at` predicate, exactly as the hand-written
/// fragment reads.
fn my_issues_scope_condition(scope: &str, user_id: Uuid) -> Condition {
    use pidash_db::entity::{issue_assignees, issue_subscribers};
    let assigned = Expr::exists(
        issue_assignees::Entity::find()
            .select_only()
            .column(issue_assignees::Column::Id)
            .filter(
                Expr::col((issue_assignees::Entity, issue_assignees::Column::IssueId))
                    .equals((issues::Entity, issues::Column::Id)),
            )
            .filter(issue_assignees::Column::AssigneeId.eq(user_id))
            .into_query(),
    );
    let subscribed = Expr::exists(
        issue_subscribers::Entity::find()
            .select_only()
            .column(issue_subscribers::Column::Id)
            .filter(
                Expr::col((
                    issue_subscribers::Entity,
                    issue_subscribers::Column::IssueId,
                ))
                .equals((issues::Entity, issues::Column::Id)),
            )
            .filter(issue_subscribers::Column::SubscriberId.eq(user_id))
            .into_query(),
    );
    match scope {
        "assigned" => Condition::all().add(assigned),
        "created" => Condition::all().add(issues::Column::CreatedById.eq(user_id)),
        _ => Condition::any()
            .add(assigned)
            .add(issues::Column::CreatedById.eq(user_id))
            .add(subscribed),
    }
}

/// The list-issues filter predicates (`ListFilterPredicates::to_sql`) as
/// SeaORM conditions over the `issues` base. UUID tokens arrive
/// pre-validated ([`normalize_list_filters`]); an unparsable token makes
/// the arm match nothing (the SQL `::uuid` cast could not fail on reachable
/// inputs).
fn list_filter_condition(filters: &ListFilterPredicates) -> Condition {
    use pidash_db::entity::issue_labels;
    let parse = |tokens: &[String]| -> Vec<Uuid> {
        tokens
            .iter()
            .filter_map(|token| Uuid::parse_str(token).ok())
            .collect()
    };
    let mut condition = Condition::all();
    if !filters.state_in.is_empty() {
        let ids = parse(&filters.state_in);
        // `issues.state_id IN (...)` — empty (all-invalid) matches nothing.
        condition = condition.add(issues::Column::StateId.is_in(ids));
    }
    if !filters.state_group_in.is_empty() {
        let groups = filters.state_group_in.clone();
        let state_ids = states::Entity::find()
            .select_only()
            .column(states::Column::Id)
            .filter(states::Column::Group.is_in(groups))
            .into_query();
        condition = condition.add(issues::Column::StateId.in_subquery(state_ids));
    }
    if !filters.priority_in.is_empty() {
        condition = condition.add(issues::Column::Priority.is_in(filters.priority_in.clone()));
    }
    if filters.parent_is_null {
        condition = condition.add(issues::Column::ParentId.is_null());
    } else if !filters.parent_in.is_empty() {
        let ids = parse(&filters.parent_in);
        condition = condition.add(issues::Column::ParentId.is_in(ids));
    }
    if !filters.label_ids_in.is_empty() {
        let ids = parse(&filters.label_ids_in);
        let labelled = Expr::exists(
            issue_labels::Entity::find()
                .select_only()
                .column(issue_labels::Column::Id)
                .filter(
                    Expr::col((issue_labels::Entity, issue_labels::Column::IssueId))
                        .equals((issues::Entity, issues::Column::Id)),
                )
                .filter(issue_labels::Column::LabelId.is_in(ids))
                .filter(issue_labels::Column::DeletedAt.is_null())
                .into_query(),
        );
        condition = condition.add(labelled);
    }
    // No predicates at all: the `to_sql` empty string adds no conjunct.
    if condition.is_empty() {
        condition = condition.add(issues::Column::Id.is_not_null());
    }
    condition
}

/// `ISSUE_FTS_SQL` as a SeaORM `Condition` (the `search_issues_page` arm):
/// FTS over name + description, `name` icontains, the `sequence_id` arm and
/// the project-identifier arm, `OR`-joined exactly as the hand-written
/// fragment reads. `ILIKE` keeps `escape_icontains` escaping through the
/// bound pattern value; the `sequence_id` arm keeps the UNNEST-array
/// membership (empty tokens match nothing, as the array form does).
fn fts_condition(query: &str) -> Condition {
    use pidash_services::assistant::tools_issues::{escape_icontains, search_sequence_tokens};
    use sea_orm::sea_query::extension::postgres::PgExpr;

    // `TO_TSVECTOR('english'::regconfig, COALESCE(name,'') || ' ' || COALESCE(description_stripped,''))`
    // and `WEBSEARCH_TO_TSQUERY('english'::regconfig, query)`: the
    // `PgFunc` OID-typed regconfig cannot carry the reviewed `'english'`
    // text cast, so the vector/query ride as `Func::cust` (the same
    // cust precedent `list_engine`/`list_grouping` keep for `row_to_json`
    // / `NULLIF` — the FTS registry literals stay inside the builder).
    let name_col = Expr::col((issues::Entity, issues::Column::Name));
    let desc_col = Expr::col((issues::Entity, issues::Column::DescriptionStripped));
    // COALESCE(name, '') || ' ' || COALESCE(description_stripped, '').
    let name_part = Func::coalesce([name_col.into_simple_expr(), Expr::val("").into()]);
    let desc_part = Func::coalesce([desc_col.into_simple_expr(), Expr::val("").into()]);
    let vector_text = name_part.concatenate(Expr::val(" ")).concatenate(desc_part);
    let vector = Func::cust(sea_orm::sea_query::Alias::new("to_tsvector"))
        .arg(Expr::val("english").cast_as(Alias::new("regconfig")))
        .arg(vector_text);
    let webquery = Func::cust(sea_orm::sea_query::Alias::new("websearch_to_tsquery"))
        .arg(Expr::val("english").cast_as(Alias::new("regconfig")))
        .arg(Expr::val(query.to_owned()));
    let fts_match = Expr::expr(vector).matches(webquery);
    let escaped = escape_icontains(query);
    let pattern = format!("%{escaped}%");
    let name_ilike = Expr::col((issues::Entity, issues::Column::Name)).ilike(pattern.clone());
    let tokens = search_sequence_tokens(query);
    let identifiers = Expr::exists(
        projects::Entity::find()
            .select_only()
            .column(projects::Column::Id)
            .filter(
                Expr::col((projects::Entity, projects::Column::Id))
                    .equals((issues::Entity, issues::Column::ProjectId)),
            )
            .filter(Expr::col((projects::Entity, projects::Column::Identifier)).ilike(pattern))
            .into_query(),
    );
    Condition::any()
        .add(fts_match)
        .add(name_ilike)
        .add(issues::Column::SequenceId.is_in(tokens))
        .add(identifiers)
}

/// The member-scoped issues base: `tools_issues::SCOPED_ISSUES_SQL`'s
/// FROM/WHERE as SeaORM joins (INNER projects/workspaces/project_members on
/// the actor, LEFT states) with the null-safe triage + archived/draft
/// conjuncts. The `project_members` membership is an INNER join with the
/// actor + active filters, exactly as the hand-written fragment reads.
fn scoped_issues_base(user_id: Uuid, slug: &str) -> sea_orm::Select<issues::Entity> {
    issues::Entity::find()
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .join(JoinType::InnerJoin, issues::Relation::Workspaces.def())
        .join(
            JoinType::InnerJoin,
            projects::Relation::ProjectMembers.def(),
        )
        .join(JoinType::LeftJoin, issues::Relation::States.def())
        .filter(issues::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(project_members::Column::MemberId.eq(user_id))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(issue_objects_condition())
}

/// One scoped-issue row decoded by name: the joined
/// project identifier and state name/group ride as aliased columns.
#[derive(Debug, FromQueryResult)]
struct IssueJoinedRow {
    id: Uuid,
    project_id: Uuid,
    project_identifier: String,
    workspace_id: Uuid,
    sequence_id: i32,
    name: String,
    parent_id: Option<Uuid>,
    state_id: Option<Uuid>,
    state_name: Option<String>,
    state_group: Option<String>,
    priority: String,
    description_stripped: Option<String>,
    created_by_id: Option<Uuid>,
    assigned_pod_id: Option<Uuid>,
    assigned_runner_id: Option<Uuid>,
    updated_at: DateTime<Utc>,
}

fn issue_row_from_joined(row: IssueJoinedRow) -> IssueRow {
    IssueRow {
        id: row.id,
        project_id: row.project_id,
        project_identifier: row.project_identifier,
        workspace_id: row.workspace_id,
        sequence_id: i64::from(row.sequence_id),
        name: row.name,
        parent_id: row.parent_id,
        state_id: row.state_id,
        state_name: row.state_name,
        state_group: row.state_group,
        priority: row.priority,
        description_stripped: row.description_stripped,
        created_by_id: row.created_by_id,
        assigned_pod_id: row.assigned_pod_id,
        assigned_runner_id: row.assigned_runner_id,
        updated_at: row.updated_at,
    }
}

/// `UPDATE issues` post-save re-read (unscoped): issues joined to projects
/// (identifier) and states (name/group), by id.
fn unscoped_issue_select(issue_id: Uuid) -> sea_orm::Select<issues::Entity> {
    issues::Entity::find()
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .join(JoinType::LeftJoin, issues::Relation::States.def())
        .filter(issues::Column::Id.eq(issue_id))
}

/// The `update_issue_row` row lock (`issues.py:336`): the `issues` row
/// alone, `FOR UPDATE` (single table — `FOR UPDATE` over the
/// `issues LEFT JOIN states` read fails at runtime: "cannot be applied
/// to the nullable side of an outer join"; Django's `select_for_update`
/// likewise locks only the issues table). The pre-save state name/group
/// are read in a second query under the same lock.
fn locked_issue_query(issue_id: Uuid) -> sea_orm::Select<issues::Entity> {
    issues::Entity::find_by_id(issue_id)
        .filter(issues::Column::DeletedAt.is_null())
        .lock_exclusive()
}

/// The identifier-lookup sequence predicate (`sequence_id = $N::int`):
/// the DB-side cast keeps out-of-int4 tails raising exactly as Django's
/// int4 bind does (miss-vs-500 parity). A Rust-side `as i32` truncation
/// would wrap instead (e.g. 4294967297 → 1) and silently look up the
/// wrong issue.
fn sequence_filter(sequence_id: i64) -> Condition {
    Condition::all().add(
        Expr::col((issues::Entity, issues::Column::SequenceId))
            .eq(Expr::val(sequence_id).cast_as(sea_orm::sea_query::Alias::new("int"))),
    )
}

fn issue_row_projection(query: sea_orm::Select<issues::Entity>) -> sea_orm::Select<issues::Entity> {
    query
        .select_only()
        .columns([
            issues::Column::Id,
            issues::Column::ProjectId,
            issues::Column::WorkspaceId,
            issues::Column::SequenceId,
            issues::Column::Name,
            issues::Column::ParentId,
            issues::Column::StateId,
            issues::Column::Priority,
            issues::Column::DescriptionStripped,
            issues::Column::CreatedById,
            issues::Column::AssignedPodId,
            issues::Column::AssignedRunnerId,
            issues::Column::UpdatedAt,
        ])
        .column_as(projects::Column::Identifier, "project_identifier")
        .column_as(states::Column::Name, "state_name")
        .column_as(states::Column::Group, "state_group")
}

/// SeaORM error mapped the way the store's `StoreError::from(sqlx::Error)`
/// arms read: database failures surface as `Db(message)`.
fn orm_err(error: DbErr) -> StoreError {
    StoreError::from(pidash_db::orm_tx::into_sqlx(error))
}

impl ToolStore for PgToolStore {
    async fn require_user(&self, deps: &AssistantDeps) -> Result<Uuid, StoreError> {
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let id: Option<Uuid> = users::Entity::find()
            .select_only()
            .column(users::Column::Id)
            .filter(users::Column::Id.eq(deps.user_id))
            .into_tuple()
            .one(&db)
            .await
            .map_err(orm_err)?;
        id.ok_or(StoreError::Missing { model: "User" })
    }

    async fn member_projects(&self, deps: &AssistantDeps) -> Result<Vec<ProjectRow>, StoreError> {
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let rows: Vec<(Uuid, String, String, Uuid, bool)> = Self::member_projects_query(deps)
            .select_only()
            .columns([
                projects::Column::Id,
                projects::Column::Identifier,
                projects::Column::Name,
                projects::Column::WorkspaceId,
                projects::Column::GuestViewAllFeatures,
            ])
            .into_tuple()
            .all(&db)
            .await
            .map_err(orm_err)?;
        Ok(rows
            .into_iter()
            .map(
                |(id, identifier, name, workspace_id, guest_view_all_features)| ProjectRow {
                    id,
                    identifier,
                    name,
                    workspace_id,
                    guest_view_all_features,
                },
            )
            .collect())
    }

    async fn member_project(
        &self,
        deps: &AssistantDeps,
        project_id: &Uuid,
    ) -> Result<Option<ProjectRow>, StoreError> {
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let row: Option<(Uuid, String, String, Uuid, bool)> = Self::member_projects_query(deps)
            .filter(projects::Column::Id.eq(*project_id))
            .limit(1)
            .offset(0)
            .select_only()
            .columns([
                projects::Column::Id,
                projects::Column::Identifier,
                projects::Column::Name,
                projects::Column::WorkspaceId,
                projects::Column::GuestViewAllFeatures,
            ])
            .into_tuple()
            .one(&db)
            .await
            .map_err(orm_err)?;
        Ok(row.map(
            |(id, identifier, name, workspace_id, guest_view_all_features)| ProjectRow {
                id,
                identifier,
                name,
                workspace_id,
                guest_view_all_features,
            },
        ))
    }

    async fn scoped_issue(
        &self,
        deps: &AssistantDeps,
        issue_id: &Uuid,
    ) -> Result<Option<IssueRow>, StoreError> {
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let row = issue_row_projection(scoped_issues_base(deps.user_id, &deps.workspace_slug))
            .filter(issues::Column::Id.eq(*issue_id))
            .into_model::<IssueJoinedRow>()
            .one(&db)
            .await
            .map_err(orm_err)?;
        Ok(row.map(issue_row_from_joined))
    }

    async fn search_issues_page(
        &self,
        deps: &AssistantDeps,
        project_id: Option<Uuid>,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<IssueRow>, StoreError> {
        use pidash_services::assistant::tools_issues::search_uses_fts;
        // `issues.search_issues :65-74`: scope, optional project
        // narrowing, FTS only for non-blank queries, `.distinct()`,
        // `-updated_at`, the lookahead window.
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let use_fts = search_uses_fts(query);
        let fts = use_fts.then(|| fts_condition(query));
        let mut query =
            issue_row_projection(scoped_issues_base(deps.user_id, &deps.workspace_slug)).distinct();
        if let Some(project) = project_id {
            query = query.filter(issues::Column::ProjectId.eq(project));
        }
        if let Some(fts) = fts {
            query = query.filter(fts);
        }
        let rows = query
            .order_by_desc(issues::Column::UpdatedAt)
            .limit((limit + 1) as u64)
            .offset(offset.max(0) as u64)
            .into_model::<IssueJoinedRow>()
            .all(&db)
            .await
            .map_err(orm_err)?;
        Ok(rows.into_iter().map(issue_row_from_joined).collect())
    }

    async fn list_issues_page(
        &self,
        deps: &AssistantDeps,
        project_id: &Uuid,
        filters: &ListFilterPredicates,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<IssueRow>, StoreError> {
        // `issues.list_issues :114-124`: project filter, compiled
        // predicates, `.distinct()`, `-updated_at`, lookahead window.
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let rows = issue_row_projection(scoped_issues_base(deps.user_id, &deps.workspace_slug))
            .distinct()
            .filter(issues::Column::ProjectId.eq(*project_id))
            .filter(list_filter_condition(filters))
            .order_by_desc(issues::Column::UpdatedAt)
            .limit((limit + 1) as u64)
            .offset(offset.max(0) as u64)
            .into_model::<IssueJoinedRow>()
            .all(&db)
            .await
            .map_err(orm_err)?;
        Ok(rows.into_iter().map(issue_row_from_joined).collect())
    }

    async fn my_issues_page(
        &self,
        deps: &AssistantDeps,
        scope: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<IssueRow>, StoreError> {
        // `issues.list_my_issues :143`: involvement filter inside the
        // member scope, `-updated_at`, lookahead window.
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let parsed = pidash_services::assistant::tools_scoping::IssueScope::parse(scope);
        let rows = issue_row_projection(scoped_issues_base(deps.user_id, &deps.workspace_slug))
            .distinct()
            .filter(my_issues_scope_condition(parsed.as_str(), deps.user_id))
            .order_by_desc(issues::Column::UpdatedAt)
            .limit((limit + 1) as u64)
            .offset(offset.max(0) as u64)
            .into_model::<IssueJoinedRow>()
            .all(&db)
            .await
            .map_err(orm_err)?;
        Ok(rows.into_iter().map(issue_row_from_joined).collect())
    }

    async fn issue_comments(&self, issue_id: &Uuid) -> Result<Vec<IssueCommentRow>, StoreError> {
        #[derive(Debug, FromQueryResult)]
        struct CommentRow {
            comment_stripped: Option<String>,
            created_at: DateTime<Utc>,
            speaker_type: String,
            actor_id: Option<Uuid>,
            actor_display_name: Option<String>,
            actor_email: Option<String>,
        }
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let rows: Vec<CommentRow> = issue_comments::Entity::find()
            .select_only()
            .columns([
                issue_comments::Column::CommentStripped,
                issue_comments::Column::CreatedAt,
                issue_comments::Column::SpeakerType,
                issue_comments::Column::ActorId,
            ])
            .column_as(users::Column::DisplayName, "actor_display_name")
            .column_as(users::Column::Email, "actor_email")
            .join(JoinType::LeftJoin, issue_comments::Relation::Users3.def())
            .filter(issue_comments::Column::IssueId.eq(*issue_id))
            .filter(issue_comments::Column::DeletedAt.is_null())
            .order_by_desc(issue_comments::Column::CreatedAt)
            .limit(10)
            .into_model()
            .all(&db)
            .await
            .map_err(orm_err)?;
        Ok(rows
            .into_iter()
            .map(|row| IssueCommentRow {
                comment_stripped: row.comment_stripped,
                created_at: row.created_at,
                speaker_type: row.speaker_type,
                actor_id: row.actor_id,
                actor_display_name: row.actor_display_name,
                actor_email: row.actor_email,
            })
            .collect())
    }

    async fn project_states(
        &self,
        deps: &AssistantDeps,
        project_id: &Uuid,
    ) -> Result<Vec<StateRow>, StoreError> {
        // `_scoping.project_states`: the `State` default manager
        // excludes soft-deleted and triage rows (`state.py:79-84`),
        // ordered by `State.Meta.ordering` (`sequence`).
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let rows: Vec<(Uuid, String, String, bool, f64)> = states::Entity::find()
            .join(JoinType::InnerJoin, states::Relation::Workspaces.def())
            .select_only()
            .columns([
                states::Column::Id,
                states::Column::Name,
                states::Column::Group,
                states::Column::Default,
                states::Column::Sequence,
            ])
            .filter(states::Column::ProjectId.eq(*project_id))
            .filter(workspaces::Column::Slug.eq(&deps.workspace_slug))
            .filter(states::Column::DeletedAt.is_null())
            .filter(states::Column::Group.ne("triage"))
            .order_by_asc(states::Column::Sequence)
            .into_tuple()
            .all(&db)
            .await
            .map_err(orm_err)?;
        Ok(rows
            .into_iter()
            .map(|(id, name, group, default, sequence)| StateRow {
                id,
                name,
                group,
                default,
                sequence,
            })
            .collect())
    }

    async fn project_labels(
        &self,
        deps: &AssistantDeps,
        project_id: &Uuid,
    ) -> Result<Vec<LabelRow>, StoreError> {
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let rows: Vec<(Uuid, String)> = labels::Entity::find()
            .join(JoinType::InnerJoin, labels::Relation::Workspaces.def())
            .select_only()
            .columns([labels::Column::Id, labels::Column::Name])
            .filter(labels::Column::ProjectId.eq(*project_id))
            .filter(workspaces::Column::Slug.eq(&deps.workspace_slug))
            .filter(labels::Column::DeletedAt.is_null())
            .order_by_asc(labels::Column::Name)
            .into_tuple()
            .all(&db)
            .await
            .map_err(orm_err)?;
        Ok(rows
            .into_iter()
            .map(|(id, name)| LabelRow { id, name })
            .collect())
    }

    async fn workspace_labels(&self, deps: &AssistantDeps) -> Result<Vec<LabelRow>, StoreError> {
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let rows: Vec<(Uuid, String)> = labels::Entity::find()
            .join(JoinType::InnerJoin, labels::Relation::Workspaces.def())
            .select_only()
            .columns([labels::Column::Id, labels::Column::Name])
            .filter(labels::Column::ProjectId.is_null())
            .filter(workspaces::Column::Slug.eq(&deps.workspace_slug))
            .filter(labels::Column::DeletedAt.is_null())
            .into_tuple()
            .all(&db)
            .await
            .map_err(orm_err)?;
        Ok(rows
            .into_iter()
            .map(|(id, name)| LabelRow { id, name })
            .collect())
    }

    async fn workspace_issue_by_identifier(
        &self,
        deps: &AssistantDeps,
        project_ident: &str,
        sequence_id: i64,
    ) -> Result<Option<IssueRow>, StoreError> {
        // The `issue_objects` manager without the member joins: slug +
        // identifier + sequence, triage/archived/draft excluded.
        // `.first()` under `Issue.Meta.ordering` (`-created_at`); the
        // DB-side `::int` cast makes out-of-int4 tails raise exactly as
        // Django's int4 bind does (miss-vs-500 parity, see
        // `sequence_filter`).
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let row = issue_row_projection(
            issues::Entity::find()
                .join(JoinType::InnerJoin, issues::Relation::Projects.def())
                .join(JoinType::InnerJoin, issues::Relation::Workspaces.def())
                .join(JoinType::LeftJoin, issues::Relation::States.def())
                .filter(issues::Column::DeletedAt.is_null())
                .filter(workspaces::Column::Slug.eq(&deps.workspace_slug))
                .filter(issue_objects_condition()),
        )
        .filter(
            Expr::expr(Func::upper(Expr::col((
                projects::Entity,
                projects::Column::Identifier,
            ))))
            .eq(Func::upper(Expr::val(project_ident.to_owned()))),
        )
        .filter(sequence_filter(sequence_id))
        .order_by_desc(issues::Column::CreatedAt)
        .into_model::<IssueJoinedRow>()
        .one(&db)
        .await
        .map_err(orm_err)?;
        Ok(row.map(issue_row_from_joined))
    }

    async fn project_members(
        &self,
        deps: &AssistantDeps,
        project_id: &Uuid,
    ) -> Result<Vec<MemberRow>, StoreError> {
        // `role` is a `PositiveSmallIntegerField` (int2): decode i16,
        // widen for the row.
        #[derive(Debug, FromQueryResult)]
        struct MemberRowDb {
            member_id: Option<Uuid>,
            display_name: Option<String>,
            email: Option<String>,
            role: i16,
        }
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let rows: Vec<MemberRowDb> = project_members::Entity::find()
            .select_only()
            .columns([
                project_members::Column::MemberId,
                project_members::Column::Role,
            ])
            .column_as(users::Column::DisplayName, "display_name")
            .column_as(users::Column::Email, "email")
            .join(
                JoinType::InnerJoin,
                project_members::Relation::Workspaces.def(),
            )
            .join(JoinType::LeftJoin, project_members::Relation::Users2.def())
            .filter(project_members::Column::ProjectId.eq(*project_id))
            .filter(workspaces::Column::Slug.eq(&deps.workspace_slug))
            .filter(project_members::Column::IsActive.eq(true))
            .filter(project_members::Column::DeletedAt.is_null())
            .order_by_desc(project_members::Column::CreatedAt)
            .into_model()
            .all(&db)
            .await
            .map_err(orm_err)?;
        // `if m.member is None: continue` (`projects.py:56-57`).
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                row.member_id.map(|member_id| MemberRow {
                    member_id,
                    display_name: row.display_name,
                    email: row.email,
                    role: i32::from(row.role),
                })
            })
            .collect())
    }

    async fn check_project_write(
        &self,
        deps: &AssistantDeps,
        project_id: &Uuid,
    ) -> Result<bool, StoreError> {
        // `check_project_role(..., [ADMIN, MEMBER])`: an active member row
        // with role 20/15, or any active membership plus workspace-admin.
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let write_role: Option<Uuid> = project_members::Entity::find()
            .select_only()
            .column(project_members::Column::Id)
            .join(
                JoinType::InnerJoin,
                project_members::Relation::Workspaces.def(),
            )
            .filter(project_members::Column::MemberId.eq(deps.user_id))
            .filter(workspaces::Column::Slug.eq(&deps.workspace_slug))
            .filter(project_members::Column::ProjectId.eq(*project_id))
            .filter(project_members::Column::Role.is_in([20i16, 15i16]))
            .filter(project_members::Column::IsActive.eq(true))
            .filter(project_members::Column::DeletedAt.is_null())
            .into_tuple()
            .one(&db)
            .await
            .map_err(orm_err)?;
        if write_role.is_some() {
            return Ok(true);
        }
        let any_member: Option<Uuid> = project_members::Entity::find()
            .select_only()
            .column(project_members::Column::Id)
            .join(
                JoinType::InnerJoin,
                project_members::Relation::Workspaces.def(),
            )
            .filter(project_members::Column::MemberId.eq(deps.user_id))
            .filter(workspaces::Column::Slug.eq(&deps.workspace_slug))
            .filter(project_members::Column::ProjectId.eq(*project_id))
            .filter(project_members::Column::IsActive.eq(true))
            .filter(project_members::Column::DeletedAt.is_null())
            .into_tuple()
            .one(&db)
            .await
            .map_err(orm_err)?;
        if any_member.is_none() {
            return Ok(false);
        }
        let admin: Option<Uuid> = pidash_db::entity::workspace_members::Entity::find()
            .select_only()
            .column(pidash_db::entity::workspace_members::Column::Id)
            .join(
                JoinType::InnerJoin,
                pidash_db::entity::workspace_members::Relation::Workspaces.def(),
            )
            .filter(pidash_db::entity::workspace_members::Column::MemberId.eq(deps.user_id))
            .filter(workspaces::Column::Slug.eq(&deps.workspace_slug))
            .filter(pidash_db::entity::workspace_members::Column::Role.eq(20i16))
            .filter(pidash_db::entity::workspace_members::Column::IsActive.eq(true))
            .filter(pidash_db::entity::workspace_members::Column::DeletedAt.is_null())
            .into_tuple()
            .one(&db)
            .await
            .map_err(orm_err)?;
        Ok(admin.is_some())
    }

    async fn agent_runs(&self, issue_id: &Uuid) -> Result<Vec<AgentRunRow>, StoreError> {
        // Full models: `build_dispatch_wait` needs the whole run for the
        // ahead-count + waiting comment (no select-only projection).
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let runs = agent_run::Entity::find()
            .filter(agent_run::Column::WorkItemId.eq(*issue_id))
            .order_by_desc(agent_run::Column::CreatedAt)
            .limit(5)
            .all(&db)
            .await
            .map_err(orm_err)?;
        let mut out = Vec::with_capacity(runs.len());
        for run in runs {
            let pinned_runner_name = match run.pinned_runner_id {
                Some(id) => runner::Entity::find_by_id(id)
                    .select_only()
                    .column(runner::Column::Name)
                    .into_tuple()
                    .one(&db)
                    .await
                    .map_err(orm_err)?
                    .flatten(),
                None => None,
            };
            let wait = pidash_services::domain::issue_assignment::build_dispatch_wait(
                &db,
                &pidash_services::domain::issue_assignment::RunWaitInputs::from(&run),
                None,
            )
            .await
            .map_err(orm_err)?;
            out.push(AgentRunRow {
                id: run.id,
                status: run.status.clone(),
                created_at: Some(chrono::DateTime::<Utc>::from(run.created_at)),
                pinned_runner_id: run.pinned_runner_id,
                pinned_runner_name,
                pin_kind: run.pin_kind.clone(),
                dispatch_wait: wait,
            });
        }
        Ok(out)
    }

    async fn list_runners_for_resolution(
        &self,
        project_id: &Uuid,
        actor_id: &Uuid,
    ) -> Result<Vec<(Uuid, String)>, StoreError> {
        use sea_orm::EntityTrait;
        let to_pod: sea_orm::RelationDef = runner::Entity::belongs_to(pod::Entity)
            .from(runner::Column::PodId)
            .to(pod::Column::Id)
            .into();
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let rows: Vec<(Uuid, String)> = runner::Entity::find()
            .select_only()
            .columns([runner::Column::Id, runner::Column::Name])
            .join(sea_orm::JoinType::InnerJoin, to_pod)
            .filter(pod::Column::ProjectId.eq(*project_id))
            .filter(runner::Column::OwnerId.eq(*actor_id))
            .filter(runner::Column::Visibility.eq(0_i16))
            .order_by_asc(runner::Column::Name)
            .into_tuple()
            .all(&db)
            .await
            .map_err(orm_err)?;
        Ok(rows)
    }

    async fn list_pods_for_resolution(
        &self,
        project_id: &Uuid,
    ) -> Result<Vec<(Uuid, String)>, StoreError> {
        use sea_orm::EntityTrait;
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let rows: Vec<(Uuid, String)> = pod::Entity::find()
            .select_only()
            .columns([pod::Column::Id, pod::Column::Name])
            .filter(pod::Column::ProjectId.eq(*project_id))
            .order_by_asc(pod::Column::Name)
            .into_tuple()
            .all(&db)
            .await
            .map_err(orm_err)?;
        Ok(rows)
    }

    async fn runner_by_id_in_project(
        &self,
        project_id: &Uuid,
        runner_id: &Uuid,
    ) -> Result<Option<(Uuid, Uuid)>, StoreError> {
        use sea_orm::EntityTrait;
        let to_pod: sea_orm::RelationDef = runner::Entity::belongs_to(pod::Entity)
            .from(runner::Column::PodId)
            .to(pod::Column::Id)
            .into();
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let row: Option<(Uuid, Uuid)> = runner::Entity::find()
            .select_only()
            .columns([runner::Column::Id, runner::Column::PodId])
            .join(sea_orm::JoinType::InnerJoin, to_pod)
            .filter(runner::Column::Id.eq(*runner_id))
            .filter(pod::Column::ProjectId.eq(*project_id))
            .into_tuple()
            .one(&db)
            .await
            .map_err(orm_err)?;
        Ok(row)
    }

    async fn pod_by_id_in_project(
        &self,
        project_id: &Uuid,
        pod_id: &Uuid,
    ) -> Result<Option<Uuid>, StoreError> {
        use sea_orm::EntityTrait;
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let row: Option<Uuid> = pod::Entity::find()
            .select_only()
            .column(pod::Column::Id)
            .filter(pod::Column::Id.eq(*pod_id))
            .filter(pod::Column::ProjectId.eq(*project_id))
            .into_tuple()
            .one(&db)
            .await
            .map_err(orm_err)?;
        Ok(row)
    }

    async fn validate_execution_target_write(
        &self,
        write: &pidash_services::domain::issue_assignment::ExecutionTargetWrite,
    ) -> Result<(), StoreError> {
        use pidash_services::assistant::tools_issues::execution_refusal_message;
        use pidash_services::domain::issue_assignment::validate_execution_target;
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        validate_execution_target(&db, write)
            .await
            .map_err(|error| match error {
                pidash_services::domain::issue_assignment::ValidateError::Field(field) => {
                    StoreError::Transition(execution_refusal_message(field.detail, field.code))
                }
                pidash_services::domain::issue_assignment::ValidateError::Db(error) => {
                    orm_err(error)
                }
            })
    }

    async fn apply_execution_target_post_commit(
        &self,
        issue_id: &Uuid,
        new_runner_id: Option<Uuid>,
        actor_id: &Uuid,
    ) -> Result<(), StoreError> {
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        pidash_services::domain::issue_assignment::apply_execution_target(
            &db,
            *issue_id,
            new_runner_id,
            *actor_id,
        )
        .await
        .map_err(orm_err)?;
        Ok(())
    }

    async fn has_blocking_run(&self, issue_id: &Uuid) -> Result<bool, StoreError> {
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        pidash_services::domain::issue_assignment::has_blocking_run(&db, *issue_id)
            .await
            .map_err(orm_err)
    }

    async fn execution_detail(
        &self,
        issue_id: &Uuid,
        actor_id: &Uuid,
    ) -> Result<ExecutionDetail, StoreError> {
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let runner_detail = match pidash_db::entity::issues::Entity::find_by_id(*issue_id)
            .select_only()
            .column(pidash_db::entity::issues::Column::AssignedRunnerId)
            .into_tuple::<Option<Uuid>>()
            .one(&db)
            .await
            .map_err(orm_err)?
            .flatten()
        {
            Some(runner_id) => {
                match runner::Entity::find_by_id(runner_id)
                    .one(&db)
                    .await
                    .map_err(orm_err)?
                {
                    Some(runner) => {
                        pidash_services::domain::issue_assignment::assigned_runner_detail(
                            &db, &runner,
                        )
                        .await
                        .map_err(orm_err)?
                    }
                    None => Value::Null,
                }
            }
            None => Value::Null,
        };
        let latest = agent_run::Entity::find()
            .filter(agent_run::Column::WorkItemId.eq(*issue_id))
            .order_by_desc(agent_run::Column::CreatedAt)
            .one(&db)
            .await
            .map_err(orm_err)?;
        let dispatch_wait = match latest {
            Some(run) => pidash_services::domain::issue_assignment::build_dispatch_wait(
                &db,
                &pidash_services::domain::issue_assignment::RunWaitInputs::from(&run),
                Some(*actor_id),
            )
            .await
            .map_err(orm_err)?,
            None => Value::Null,
        };
        Ok(ExecutionDetail {
            runner_detail,
            dispatch_wait,
        })
    }

    async fn list_project_runners(
        &self,
        project_id: &Uuid,
        actor_id: &Uuid,
        assignable_for: Option<&str>,
    ) -> Result<Vec<Value>, StoreError> {
        use pidash_services::runner_enroll::queries::manage_reads;
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        // The web list is workspace-scoped; the tool narrows by project.
        let workspace_id = projects::Entity::find_by_id(*project_id)
            .select_only()
            .column(projects::Column::WorkspaceId)
            .into_tuple::<Uuid>()
            .one(&db)
            .await
            .map_err(orm_err)?
            .ok_or(StoreError::Missing { model: "Project" })?;
        // `?assignable_for=<issue-id | "new">`: a garbage id behaves as
        // "new" (Django's `except (ValueError, TypeError)` arm).
        let assignable_for: Option<Option<Uuid>> = match assignable_for {
            None => None,
            Some("new") => Some(None),
            Some(raw) => Some(raw.parse::<Uuid>().ok()),
        };
        let rows = manage_reads::runner_list(
            &db,
            workspace_id,
            *actor_id,
            0_i16,
            None,
            None,
            Some(*project_id),
        )
        .await
        .map_err(orm_err)?;
        let mut bodies = Vec::with_capacity(rows.len());
        for row in &rows {
            let text = crate::runner_enroll::manage::render_runner_row_ctx(
                &self.pool,
                row,
                Some(*actor_id),
                assignable_for,
            )
            .await
            .map_err(|_| StoreError::Db("runner row render failed".to_owned()))?;
            let value: Value =
                serde_json::from_str(&text).map_err(|error| StoreError::Db(error.to_string()))?;
            bodies.push(value);
        }
        Ok(bodies)
    }

    async fn scoped_issue_by_identifier(
        &self,
        deps: &AssistantDeps,
        project_ident: &str,
        sequence_id: i64,
    ) -> Result<Option<IssueRow>, StoreError> {
        // `.first()` under `Issue.Meta.ordering` (`-created_at`);
        // the DB-side `::int` cast keeps int4-range parity with Django's
        // bind (out-of-range tails raise instead of missing, see
        // `sequence_filter`).
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let row = issue_row_projection(scoped_issues_base(deps.user_id, &deps.workspace_slug))
            .filter(
                Expr::expr(Func::upper(Expr::col((
                    projects::Entity,
                    projects::Column::Identifier,
                ))))
                .eq(Func::upper(Expr::val(project_ident.to_owned()))),
            )
            .filter(sequence_filter(sequence_id))
            .order_by_desc(issues::Column::CreatedAt)
            .into_model::<IssueJoinedRow>()
            .one(&db)
            .await
            .map_err(orm_err)?;
        Ok(row.map(issue_row_from_joined))
    }

    async fn pair_rows(&self, a_id: &Uuid, b_id: &Uuid) -> Result<Vec<PairRow>, StoreError> {
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let rows: Vec<(Uuid, Uuid, Uuid, String)> = issue_relations::Entity::find()
            .select_only()
            .columns([
                issue_relations::Column::Id,
                issue_relations::Column::IssueId,
                issue_relations::Column::RelatedIssueId,
                issue_relations::Column::RelationType,
            ])
            .filter(
                Condition::any()
                    .add(
                        Condition::all()
                            .add(issue_relations::Column::IssueId.eq(*a_id))
                            .add(issue_relations::Column::RelatedIssueId.eq(*b_id)),
                    )
                    .add(
                        Condition::all()
                            .add(issue_relations::Column::IssueId.eq(*b_id))
                            .add(issue_relations::Column::RelatedIssueId.eq(*a_id)),
                    ),
            )
            .filter(issue_relations::Column::DeletedAt.is_null())
            .into_tuple()
            .all(&db)
            .await
            .map_err(orm_err)?;
        Ok(rows
            .into_iter()
            .map(|(id, issue_id, related_issue_id, relation_type)| PairRow {
                id,
                issue_id,
                related_issue_id,
                relation_type,
            })
            .collect())
    }

    async fn insert_relation(
        &self,
        issue_id: &Uuid,
        related_id: &Uuid,
        stored_type: &str,
        project_id: &Uuid,
        workspace_id: &Uuid,
        actor_id: &Uuid,
        now: &DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        // `relations.relate :196-207`: the INSERT, with the
        // unique-violation arm (`IntegrityError` → re-read) as `Ok(false)`.
        let row = relation_domain::RelationRow {
            id: Uuid::new_v4(),
            created_at: *now,
            updated_at: *now,
            created_by: Some(*actor_id),
            updated_by: None,
            project_id: *project_id,
            workspace_id: *workspace_id,
            issue_id: *issue_id,
            related_issue_id: *related_id,
            relation_type: stored_type,
        };
        let outcome =
            relation_domain::relate_row(&pidash_db::pool::sea_orm_connection(&self.pool), &row)
                .await;
        match outcome {
            Ok(_) => Ok(true),
            Err(error)
                if error
                    .as_database_error()
                    .and_then(|db| db.code())
                    .as_deref()
                    == Some("23505") =>
            {
                Ok(false)
            }
            Err(error) => Err(StoreError::from(error)),
        }
    }

    async fn delete_relation(
        &self,
        row_id: &Uuid,
        actor_id: &Uuid,
        now: &DateTime<Utc>,
    ) -> Result<(), StoreError> {
        // `row.delete()` (`relations.unrelate :258-260`): soft-delete
        // plus the related-objects sweep enqueue. Best-effort by repo
        // precedent (unlike `.delay()`, which propagates a broker
        // failure): a queue failure must not fail the op.
        relation_domain::soft_delete_relation_row(
            &pidash_db::pool::sea_orm_connection(&self.pool),
            *row_id,
            *actor_id,
            *now,
            *now,
        )
        .await?;
        let mut kwargs = serde_json::Map::with_capacity(1);
        kwargs.insert("using".to_owned(), Value::Null);
        enqueue_best_effort(
            &self.pool,
            pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
            vec![
                Value::String("db".to_owned()),
                // `self._meta.model_name` (`mixins.py:78`): lowercase.
                Value::String("issuerelation".to_owned()),
                Value::String(row_id.to_string()),
            ],
            kwargs,
        )
        .await;
        Ok(())
    }

    async fn enqueue_relation_activity(
        &self,
        activity: &RelationActivity,
    ) -> Result<(), StoreError> {
        // `relations._log_activity`: a 9-key kwargs map
        // (`type`, `current_instance`, `actor_id`, `issue_id`,
        // `project_id`, `epoch`, `requested_data`, `notification`,
        // `origin`) — the shared builder plus the relate/unrelate
        // trailing `notification`, no `origin`.
        let mut kwargs = pidash_services::v1_work_items::tasks::issue_activity_kwargs(
            &activity.activity_type,
            activity.requested_data.as_deref(),
            &activity.actor_id.to_string(),
            &activity.issue_id.to_string(),
            &activity.project_id.to_string(),
            activity.current_instance.as_deref(),
            activity.epoch,
        );
        kwargs.insert("notification".to_owned(), Value::Bool(true));
        enqueue_best_effort(
            &self.pool,
            pidash_services::v1_work_items::tasks::ISSUE_ACTIVITY_TASK,
            Vec::new(),
            kwargs,
        )
        .await;
        Ok(())
    }

    async fn grouped_relation_rows(
        &self,
        issue_id: &Uuid,
        workspace_id: &Uuid,
    ) -> Result<Vec<PairRow>, StoreError> {
        // Every live row touching the issue in its workspace, self-pairs
        // excluded (`GROUPED_RELATIONS_SQL`).
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let rows: Vec<(Uuid, Uuid, Uuid, String)> = issue_relations::Entity::find()
            .select_only()
            .columns([
                issue_relations::Column::Id,
                issue_relations::Column::IssueId,
                issue_relations::Column::RelatedIssueId,
                issue_relations::Column::RelationType,
            ])
            .filter(
                Condition::any()
                    .add(issue_relations::Column::IssueId.eq(*issue_id))
                    .add(issue_relations::Column::RelatedIssueId.eq(*issue_id)),
            )
            .filter(issue_relations::Column::WorkspaceId.eq(*workspace_id))
            .filter(
                Condition::all()
                    .add(issue_relations::Column::IssueId.eq(*issue_id))
                    .add(issue_relations::Column::RelatedIssueId.eq(*issue_id))
                    .not(),
            )
            .filter(issue_relations::Column::DeletedAt.is_null())
            .into_tuple()
            .all(&db)
            .await
            .map_err(orm_err)?;
        Ok(rows
            .into_iter()
            .map(|(id, issue_id, related_issue_id, relation_type)| PairRow {
                id,
                issue_id,
                related_issue_id,
                relation_type,
            })
            .collect())
    }

    async fn visible_grouped_issues(
        &self,
        deps: &AssistantDeps,
        ids: &[Uuid],
        workspace_id: &Uuid,
    ) -> Result<Vec<GroupedIssueRow>, StoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        #[derive(Debug, FromQueryResult)]
        struct GroupedRead {
            id: Uuid,
            project_identifier: String,
            sequence_id: i32,
            name: String,
            state_name: Option<String>,
            state_group: Option<String>,
        }
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let rows: Vec<GroupedRead> = scoped_issues_base(deps.user_id, &deps.workspace_slug)
            .select_only()
            .columns([
                issues::Column::Id,
                issues::Column::SequenceId,
                issues::Column::Name,
            ])
            .column_as(projects::Column::Identifier, "project_identifier")
            .column_as(states::Column::Name, "state_name")
            .column_as(states::Column::Group, "state_group")
            .filter(issues::Column::Id.is_in(ids.to_vec()))
            .filter(issues::Column::WorkspaceId.eq(*workspace_id))
            .into_model()
            .all(&db)
            .await
            .map_err(orm_err)?;
        Ok(rows
            .into_iter()
            .map(|row| GroupedIssueRow {
                id: row.id,
                project_identifier: row.project_identifier,
                sequence_id: i64::from(row.sequence_id),
                name: row.name,
                state_name: row.state_name,
                state_group: row.state_group,
            })
            .collect())
    }

    async fn issue_parent_id(&self, issue_id: &Uuid) -> Result<Option<Uuid>, StoreError> {
        // `Issue.objects.filter(pk).values_list("parent_id").first()`:
        // the plain manager (soft-delete only, triage included).
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let parent: Option<Option<Uuid>> = issues::Entity::find()
            .select_only()
            .column(issues::Column::ParentId)
            .filter(issues::Column::Id.eq(*issue_id))
            .filter(issues::Column::DeletedAt.is_null())
            .into_tuple()
            .one(&db)
            .await
            .map_err(orm_err)?;
        Ok(parent.flatten())
    }

    async fn create_issue_row(
        &self,
        input: &NewIssueInput,
        now: &DateTime<Utc>,
    ) -> Result<IssueRow, StoreError> {
        let mut tx = self.pool.begin().await?;
        // The tool's `Project.objects.select_for_update().get(...)`
        // (`issues.py:259`); missing is `DoesNotExist` → internal.
        let locked: Option<Uuid> = projects::Entity::find_by_id(input.project_id)
            .filter(projects::Column::DeletedAt.is_null())
            .lock_exclusive()
            .select_only()
            .column(projects::Column::Id)
            .into_tuple()
            .one(&pidash_db::orm_tx::TxConn::new(&mut tx))
            .await
            .map_err(orm_err)?;
        if locked.is_none() {
            return Err(StoreError::Missing { model: "Project" });
        }
        // The tool's `MAX(sequence_id)+1` read (`issues.py:260-264`) is
        // intentionally not repeated: `Issue.save` overwrites the
        // passed `sequence_id` from `issue_sequences` (`issue.py:313`),
        // so the read is unobservable (a lock-free SELECT).
        // raw-sql: `pg_advisory_xact_lock` is the D-37 advisory-lock
        // statement the repair precedent ports for `Issue.save`'s
        // sequence allocation; it is not an entity query.
        {
            let key = pidash_db::ops::repair::convert_uuid_to_integer(&input.project_id);
            let stmt = sea_orm::sea_query::Query::select()
                .expr(
                    sea_orm::sea_query::Func::cust(sea_orm::sea_query::Alias::new(
                        "pg_advisory_xact_lock",
                    ))
                    .arg(key),
                )
                .to_owned();
            pidash_db::orm_tx::TxConn::new(&mut tx)
                .execute_unprepared(&stmt.to_string(sea_orm::sea_query::PostgresQueryBuilder)) // raw-sql: pg_advisory_xact_lock is the D-37 repair-precedent advisory lock; not an entity query.
                .await
                .map_err(orm_err)?;
        }
        // `IssueSequence.objects` is the soft-delete manager, and
        // `sequence` is a `PositiveBigIntegerField` (int8).
        let last: Option<Option<i64>> = issue_sequences::Entity::find()
            .select_only()
            .column_as(issue_sequences::Column::Sequence.max(), "m")
            .filter(issue_sequences::Column::ProjectId.eq(input.project_id))
            .filter(issue_sequences::Column::DeletedAt.is_null())
            .into_tuple()
            .one(&pidash_db::orm_tx::TxConn::new(&mut tx))
            .await
            .map_err(orm_err)?;
        let sequence: i64 = last
            .flatten()
            .map(|value| value.saturating_add(1))
            .unwrap_or(1);
        // `Issue.save` None-state resolution (`issue.py:286-301`):
        // default non-triage state, else the first non-triage row
        // (`.first()` under `State.Meta.ordering`, so lowest
        // `sequence`), else None. Both the `is_triage` filter from the
        // query expression and the `StateManager` group exclusion apply:
        // seeded Triage rows carry `is_triage = FALSE` (the project seed
        // never sets it), so the column alone would admit them.
        let state_id = match input.state_id {
            Some(id) => Some(id),
            None => {
                let default: Option<Uuid> = states::Entity::find()
                    .select_only()
                    .column(states::Column::Id)
                    .filter(states::Column::ProjectId.eq(input.project_id))
                    .filter(states::Column::IsTriage.eq(false))
                    .filter(states::Column::DeletedAt.is_null())
                    .filter(states::Column::Group.ne("triage"))
                    .filter(states::Column::Default.eq(true))
                    .order_by_asc(states::Column::Sequence)
                    .into_tuple()
                    .one(&pidash_db::orm_tx::TxConn::new(&mut tx))
                    .await
                    .map_err(orm_err)?;
                match default {
                    Some(id) => Some(id),
                    None => states::Entity::find()
                        .select_only()
                        .column(states::Column::Id)
                        .filter(states::Column::ProjectId.eq(input.project_id))
                        .filter(states::Column::IsTriage.eq(false))
                        .filter(states::Column::DeletedAt.is_null())
                        .filter(states::Column::Group.ne("triage"))
                        .order_by_asc(states::Column::Sequence)
                        .into_tuple()
                        .one(&pidash_db::orm_tx::TxConn::new(&mut tx))
                        .await
                        .map_err(orm_err)?,
                }
            }
        };
        // `completed_at` (`issue.py:302-309`): only when the caller
        // passed a state — a None resolved above skips the stamp (the
        // `else` quirk).
        let mut completed_at: Option<DateTime<Utc>> = None;
        let mut state_name: Option<String> = None;
        let mut state_group: Option<String> = None;
        if let Some(id) = state_id {
            let row: Option<(String, String)> = states::Entity::find()
                .select_only()
                .columns([states::Column::Name, states::Column::Group])
                .filter(states::Column::Id.eq(id))
                .into_tuple()
                .one(&pidash_db::orm_tx::TxConn::new(&mut tx))
                .await
                .map_err(orm_err)?;
            if let Some((name, group)) = row {
                if input.state_id.is_some() && group == "completed" {
                    completed_at = Some(*now);
                }
                state_name = Some(name);
                state_group = Some(group);
            }
        }
        // `assigned_pod` (`issue.py:276-284`): the project's default pod —
        // unless the tool asked for a dedicated target, which the
        // executor already resolved (the pod follows the runner, §7.2).
        let assigned_pod: Option<Uuid> = match input.assigned_pod_id {
            Some(pod_id) => Some(pod_id),
            None => pod::Entity::find()
                .select_only()
                .column(pod::Column::Id)
                .filter(pod::Column::ProjectId.eq(input.project_id))
                .filter(pod::Column::IsDefault.eq(true))
                .filter(pod::Column::DeletedAt.is_null())
                .into_tuple()
                .one(&pidash_db::orm_tx::TxConn::new(&mut tx))
                .await
                .map_err(orm_err)?,
        };
        // `sort_order` (`issue.py:333-340`): max in (project, state) +
        // 10000, else the 65535 default. `Issue.objects` (soft-delete
        // only) — including triage/archived/draft rows.
        let largest: Option<Option<f64>> = {
            let query = issues::Entity::find()
                .select_only()
                .column_as(issues::Column::SortOrder.max(), "m")
                .filter(issues::Column::ProjectId.eq(input.project_id))
                .filter(issues::Column::DeletedAt.is_null());
            let query = match state_id {
                Some(id) => query.filter(issues::Column::StateId.eq(id)),
                None => query.filter(issues::Column::StateId.is_null()),
            };
            query
                .into_tuple()
                .one(&pidash_db::orm_tx::TxConn::new(&mut tx))
                .await
                .map_err(orm_err)?
        };
        let sort_order = largest
            .flatten()
            .map(|value| value + 10000.0)
            .unwrap_or(65535.0);
        // `description_stripped` (`issue.py:341-350`): the MLStripper
        // recompute, None for empty HTML.
        let stripped: Option<String> = if input.description_html.is_empty() {
            None
        } else {
            Some(pidash_db::app_pages::strip::ml_strip_tags(
                &input.description_html,
            ))
        };
        let issue_id = Uuid::new_v4();
        issue_domain::insert_tool_issue(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            &issue_domain::ToolIssueInsert {
                issue_id,
                now: *now,
                name: &input.name,
                description_html: &input.description_html,
                description_stripped: stripped.as_deref(),
                priority: &input.priority,
                sequence_id: sequence,
                sort_order,
                completed_at,
                created_via: &input.created_via,
                created_by: input.created_by,
                project_id: input.project_id,
                workspace_id: input.workspace_id,
                parent_id: input.parent_id,
                state_id,
                pod_id: assigned_pod,
                runner_id: input.assigned_runner_id,
            },
        )
        .await?;
        issue_domain::insert_tool_issue_sequence(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            &issue_domain::ToolIssueSequence {
                project_id: input.project_id,
                workspace_id: input.workspace_id,
                issue_id,
                sequence,
                created_by: input.created_by,
                now: *now,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(IssueRow {
            id: issue_id,
            project_id: input.project_id,
            project_identifier: input.project_identifier.clone(),
            workspace_id: input.workspace_id,
            sequence_id: sequence,
            name: input.name.clone(),
            parent_id: input.parent_id,
            state_id,
            state_name,
            state_group,
            priority: input.priority.clone(),
            description_stripped: stripped,
            created_by_id: Some(input.created_by),
            assigned_pod_id: assigned_pod,
            assigned_runner_id: input.assigned_runner_id,
            updated_at: *now,
        })
    }

    async fn update_issue_row(
        &self,
        patch: &IssuePatch,
        issue_id: &Uuid,
        to_state: Option<pidash_services::orchestration::creation::StateView>,
        actor_id: &Uuid,
        now: &DateTime<Utc>,
    ) -> Result<(IssueRow, Option<TransitionOutcome>), StoreError> {
        use pidash_jobs::admission::RedisAdmissionCache;
        use pidash_jobs::orchestration::{LiveCreationStore, StoreDeps};
        use pidash_services::orchestration::entries::{
            handle_issue_state_transition, TransitionRequest,
        };
        // The creator's BYOK key presence for the managed-LLM profile
        // closure (D-11 `drive_transition`); a pre-tx pool read, as the
        // drive's was before fusing.
        let key_row: Option<Option<Vec<u8>>> = assistant_user_llm_config::Entity::find()
            .select_only()
            .column(assistant_user_llm_config::Column::ApiKeyEncrypted)
            .filter(assistant_user_llm_config::Column::UserId.eq(*actor_id))
            .into_tuple()
            .one(&pidash_db::pool::sea_orm_connection(&self.pool))
            .await
            .map_err(orm_err)?;
        let has_key = key_row.flatten().is_some_and(|key| !key.is_empty());
        let actor = *actor_id;
        let cloud = self.cloud.clone();
        let managed = self.managed.clone();
        let toolsets = Arc::clone(&self.toolsets);
        let now_unix_secs = now.timestamp();
        // Built up to three times (the probe, drive, and bounce stores
        // each take owned deps).
        let make_deps = || StoreDeps {
            cloud: cloud.clone(),
            managed: managed.clone(),
            cache: RedisAdmissionCache::from_env(),
            toolsets: Arc::clone(&toolsets),
            has_usable_llm_config: move |creator: Uuid| creator == actor && has_key,
            llm_profile: move |user: Option<Uuid>| {
                let key = user == Some(actor) && has_key;
                let profile = pidash_services::assistant::seams::agent_model_profile_for_user(key);
                pidash_services::dispatch::LlmProfile {
                    available: profile.available,
                    reason_code: profile.reason_code,
                }
            },
            now_unix_secs,
        };
        // One transaction for lock + save + drive (`issues.py:334-381`
        // runs all three under one `transaction.atomic()`): a mid-drive
        // failure rolls the edit back. The `pidash_db` tx starts here
        // so the save runs on `tx.inner()` and the drive stores take
        // the same tx by value (the `tools_exec` precedent).
        let mut tx = pidash_db::tx::Transaction::begin(&self.pool).await?;
        // `Issue.objects.select_for_update().get(pk)` (`issues.py:336`);
        // missing is `DoesNotExist` → internal. The lock is the single
        // `issues` row; the pre-save name/group (`from_state =
        // locked.state`, `issues.py:339`) are read under the same lock
        // in a second query — a join-lock over the LEFT JOIN states
        // read fails at runtime, and a pre-lock read may be stale
        // under a concurrent edit.
        let locked = locked_issue_query(*issue_id)
            .one(&pidash_db::orm_tx::TxConn::new(tx.inner()))
            .await
            .map_err(orm_err)?;
        let Some(locked) = locked else {
            return Err(StoreError::Missing { model: "Issue" });
        };
        let from_id = locked.state_id;
        let (from_name, from_group) = match from_id {
            Some(state_id) => {
                let row: Option<(String, String)> = states::Entity::find_by_id(state_id)
                    .select_only()
                    .columns([states::Column::Name, states::Column::Group])
                    .into_tuple()
                    .one(&pidash_db::orm_tx::TxConn::new(tx.inner()))
                    .await
                    .map_err(orm_err)?;
                match row {
                    Some((name, group)) => (Some(name), Some(group)),
                    None => (None, None),
                }
            }
            None => (None, None),
        };
        // `locked.save(update_fields=...)` (`issues.py:337-361`):
        // exactly the tool's columns (the write itself lives in
        // `domain::issues`, on this same fused transaction).
        issue_domain::update_tool_issue(
            &pidash_db::orm_tx::TxConn::new(tx.inner()),
            &issue_domain::ToolIssueUpdate {
                issue_id: *issue_id,
                name: patch.name.as_deref(),
                description_html: patch.description_html.as_deref(),
                priority: patch.priority.as_deref(),
                state_given: patch.state_given,
                state_id: patch.state_id,
                parent_touched: patch.parent_touched,
                parent_id: patch.parent_id,
                pod_touched: patch.pod_touched,
                pod_id: patch.pod_id,
                updated_by_id: *actor_id,
                now: *now,
            },
        )
        .await?;
        // Post-save read (unscoped: scope was verified pre-lock; this
        // mirrors the in-memory `locked` row plus joined names).
        let row = issue_row_projection(unscoped_issue_select(*issue_id))
            .into_model::<IssueJoinedRow>()
            .one(&pidash_db::orm_tx::TxConn::new(tx.inner()))
            .await
            .map_err(orm_err)?;
        let mut row = row
            .map(issue_row_from_joined)
            .ok_or(StoreError::Missing { model: "Issue" })?;
        // The execution target after the save, before the dispatch
        // (PDASHOSS01-350): the Django order is save → apply → drive.
        // A runner change stamps through the shared service on this
        // transaction; a pod-only change clears a dedicated runner
        // (§13.1) via the same service.
        if patch.apply_runner {
            pidash_services::domain::issue_assignment::apply_execution_target_in(
                &pidash_db::orm_tx::TxConn::new(tx.inner()),
                *issue_id,
                patch.new_runner_id,
                *actor_id,
            )
            .await
            .map_err(orm_err)?;
        } else if patch.pod_touched && row.assigned_runner_id.is_some() {
            pidash_services::domain::issue_assignment::clear_dedicated_runner(
                &pidash_db::orm_tx::TxConn::new(tx.inner()),
                *issue_id,
                "pod changed",
                Some(*actor_id),
            )
            .await
            .map_err(orm_err)?;
        }
        // Re-read after a target apply so the brief reports the
        // stamped assignment (the apply re-stamps runner + pod).
        if patch.apply_runner || (patch.pod_touched && row.assigned_runner_id.is_some()) {
            let fresh = issue_row_projection(unscoped_issue_select(*issue_id))
                .into_model::<IssueJoinedRow>()
                .one(&pidash_db::orm_tx::TxConn::new(tx.inner()))
                .await
                .map_err(orm_err)?;
            row = fresh.map(issue_row_from_joined).unwrap_or(row);
        }
        // The save-time state: the completion-comment decision below
        // stays keyed to the save (`to_state`), exactly as Django's
        // save-time receiver — the post-drive re-read must not move
        // it (PIDASHCONV-1444).
        let saved_state_id = row.state_id;
        // `to_state is not None and from_state != to_state`
        // (`issues.py:373`): the ORM instances compare by pk, and a
        // stateless issue always counts as changed.
        let changed = to_state.as_ref().is_some_and(|to| from_id != Some(to.id));
        let (mut tx, drive_outcome, post_commit) = if changed {
            let to = to_state.expect("changed implies a target");
            // Name/group come from the locked row (`from_state =
            // locked.state`, `issues.py:339`), not the pre-lock read —
            // a concurrent edit may have moved the issue meanwhile. A
            // dangling state FK decodes name/group as `""`.
            let from_view = from_id.map(|id| pidash_services::orchestration::creation::StateView {
                id,
                name: from_name.unwrap_or_default(),
                group: from_group.unwrap_or_default(),
            });
            // `jitter_secs: 0.0`: the L5 quirk-5 precedent (the v1
            // human-move drivers pass 0.0 as well). `moved_by_run:
            // None`: the tools are human-equivalent moves, so the
            // entry-run builder is reachable. `created_by:
            // Some(actor)`: the update path runs impersonated.
            let request = TransitionRequest {
                issue_id: *issue_id,
                from_state: from_view,
                to_state: Some(to),
                actor: Some(*actor_id),
                dispatch_immediate: true,
                moved_by_run: None,
                now: *now,
                jitter_secs: 0.0,
                created_by: Some(*actor_id),
                pod_once: false,
            };
            // Preflight on the fused transaction: Python's bounce
            // atomic nests inside the outer atomic, so the bounce reads
            // the saved row and its writes roll back with the edit. A
            // pool-side preflight (as `TurnPreflight` runs on the
            // dispatch path) cannot run here — its bounce UPDATE would
            // block on this transaction's row lock with no statement
            // timeout to break the wait — and the driver lends the tx
            // to only one seam at a time, so the fused stores below
            // take it in turn: probe the decision behind a savepoint
            // (the probe's bounce writes roll back), drive with a
            // fixed-decision stub, and — only when stubbed false —
            // re-run the services preflight after the drive, so the
            // real bounce lands post-reconcile, exactly as in Python
            // (the re-run re-derives its reason from current reads). A
            // concurrent eligibility flip between probe and re-run
            // degrades to an armed ticker the next tick covers.
            let matcher =
                crate::v1_work_items::handlers_actions::LiveMatcher::new(self.pool.clone());
            let triggered_by =
                pidash_db::dispatch::status::AgentRunTrigger::StateTransition.value();
            self.probe_savepoint(&mut tx).await?;
            let mut probe_store = LiveCreationStore::new(tx, &self.pool, make_deps());
            // `resolve_pod_for_issue` reads issue + pods only (no
            // ticker), so probing pre-reconcile decides identically to
            // Python's post-reconcile preflight. `None` still stubs
            // false: the drive re-resolves the pod itself, and only
            // consults the stub when it finds one (the appear-race) —
            // the re-run then derives the true decision.
            let probed_pod = pidash_services::orchestration::creation::resolve_pod_for_issue(
                &mut probe_store,
                *issue_id,
            )
            .await
            .map_err(|error| StoreError::Transition(error.to_string()))?;
            let proceed = match probed_pod {
                Some(pod_id) => {
                    let probe =
                        pidash_services::orchestration::dispatch::preflight_eligibility_or_bounce(
                            &mut probe_store,
                            *issue_id,
                            Some(actor),
                            pod_id,
                            triggered_by,
                            &matcher,
                            *now,
                            0.0,
                        )
                        .await
                        .map_err(|error| StoreError::Transition(error.to_string()))?;
                    probe.proceed
                }
                None => false,
            };
            // The probe's outboxes stay empty (the bounce dispatches,
            // defers, and finalizes nothing), so only the tx is
            // recovered; the savepoint rollback erases the probe's
            // writes, and its log lines are dropped (the re-run emits
            // the real bounce's lines once).
            let (mut tx, _, _, _, _, _) = probe_store.into_parts();
            self.rollback_probe_savepoint(&mut tx).await?;
            let mut drive_store = LiveCreationStore::new(tx, &self.pool, make_deps());
            let mut stub = FixedPreflight {
                proceed,
                consulted: false,
            };
            let outcome =
                handle_issue_state_transition(&mut drive_store, &mut stub, &request).await;
            let (tx, _, cache, mut dispatches, mut deferred, mut terminal_effects) =
                drive_store.into_parts();
            let outcome = outcome.map_err(|error| match error {
                pidash_services::orchestration::entries::EntriesError::Store(store) => {
                    StoreError::Transition(store.to_string())
                }
                other => StoreError::Transition(other.to_string()),
            })?;
            // Only a consulted stub owes the re-run: early returns
            // (parked, queued, no creator/pod, race lost) consult
            // nothing, and Python bounces on none of them.
            let tx = if !proceed && stub.consulted {
                // The re-run IS the real bounce when it still says so
                // (same services fn, same tx, post-reconcile state);
                // when eligibility flipped concurrently it proceeds and
                // the armed ticker covers the entry on the next tick.
                // Fresh pod first (closest to Python's read position),
                // the probed id when the pod vanished mid-drive (as
                // Python's single read would have seen it).
                let mut bounce_store = LiveCreationStore::new(tx, &self.pool, make_deps());
                let fresh_pod = pidash_services::orchestration::creation::resolve_pod_for_issue(
                    &mut bounce_store,
                    *issue_id,
                )
                .await
                .map_err(|error| StoreError::Transition(error.to_string()))?;
                if let Some(pod_id) = fresh_pod.or(probed_pod) {
                    let rerun =
                        pidash_services::orchestration::dispatch::preflight_eligibility_or_bounce(
                            &mut bounce_store,
                            *issue_id,
                            Some(actor),
                            pod_id,
                            triggered_by,
                            &matcher,
                            *now,
                            0.0,
                        )
                        .await
                        .map_err(|error| StoreError::Transition(error.to_string()))?;
                    for log in &rerun.logs {
                        if log.level == "WARNING" {
                            tracing::warn!(target: "pidash.preflight", "{}", log.message);
                        } else {
                            tracing::info!(target: "pidash.preflight", "{}", log.message);
                        }
                    }
                } else {
                    tracing::warn!(
                        target: "pidash.preflight",
                        "turn fused drive: pod vanished mid-drive for issue {issue_id}, skipping bounce",
                    );
                }
                let (tx, _, _, redispatches, redeferred, reterminal) = bounce_store.into_parts();
                dispatches.extend(redispatches);
                deferred.extend(redeferred);
                terminal_effects.extend(reterminal);
                tx
            } else {
                tx
            };
            (
                tx,
                Some(TransitionOutcome {
                    created_run_id: outcome.created_run,
                    reason: outcome.reason,
                }),
                Some((cache, dispatches, deferred, terminal_effects)),
            )
        } else {
            (tx, None, None)
        };
        // The drive may have bounced the issue to a Backlog target
        // (`scheduling.py:1111-1113` mutates the in-memory issue
        // Django briefs at `issues.py:388`): re-read on this same
        // transaction so the returned row is post-drive. The row is
        // locked and the bounce wrote here, so a miss is unreachable;
        // `Missing` keeps the file's lost-race shape (PIDASHCONV-1444).
        if changed {
            let fresh = issue_row_projection(unscoped_issue_select(*issue_id))
                .into_model::<IssueJoinedRow>()
                .one(&pidash_db::orm_tx::TxConn::new(tx.inner()))
                .await
                .map_err(orm_err)?;
            row = fresh
                .map(issue_row_from_joined)
                .ok_or(StoreError::Missing { model: "Issue" })?;
        }
        tx.commit().await?;
        // `github_signals.trigger_completion_comment`
        // (`bgtasks/github_signals.py:44-75`): a save moving a mirrored
        // issue into a completed-group state enqueues the provider's
        // `post_completion_comment`. Django fires this from
        // `Issue.save()` (`issues.py:369`), so the tool path gets it
        // for free; here signals are explicit calls after commit.
        // `changed` is the transition gate (a target state was given
        // and differs from the locked pre-save state); the dispatch
        // helper re-checks the completed group, the mirror order (git
        // before github) and the already-commented guard. Best-effort
        // like the relation enqueues: the edit already committed, so
        // a lookup or queue failure must not fail the tool.
        if changed {
            self.enqueue_completion_comment(issue_id, from_id.as_ref(), saved_state_id.as_ref())
                .await;
        }
        // Post-commit dispatch drain (the `dispatch_coding_run` drain
        // shape, duplicated: unlike D-11 — whose `moved_by_run` makes
        // the outbox unreachable — human moves dispatch here, after the
        // commit, so the runner read sees the rows).
        if let Some((cache, dispatches, deferred, terminal_effects)) = post_commit {
            // A finalized run is a normal outcome, not an error: the
            // prompt-build failure path (`service.py:820-832`)
            // finalizes the run FAILED and returns `render-failed`
            // with no exception — Django commits the whole atomic
            // and the tool briefs normally (PIDASHCONV-1571).
            for run_id in &terminal_effects {
                drain_finalized_run(&self.pool, *run_id).await;
            }
            for run_id in dispatches {
                pidash_jobs::dispatch::dispatch_agent_run(&self.pool, &self.cloud, run_id, *now)
                    .await
                    .map_err(StoreError::from)?;
            }
            for consume in &deferred {
                pidash_services::dispatch::admission::consume_admission_token(&cache, consume);
            }
        }
        Ok((row, drive_outcome))
    }

    #[allow(clippy::too_many_arguments)]
    async fn drive_transition(
        &self,
        issue_id: &Uuid,
        from_state: Option<pidash_services::orchestration::creation::StateView>,
        to_state: pidash_services::orchestration::creation::StateView,
        actor_id: &Uuid,
        created_by: Option<Uuid>,
        now: &DateTime<Utc>,
        pod_once: bool,
    ) -> Result<TransitionOutcome, StoreError> {
        use pidash_jobs::admission::RedisAdmissionCache;
        use pidash_jobs::orchestration::{LiveCreationStore, StoreDeps};
        use pidash_services::orchestration::entries::{
            handle_issue_state_transition, TransitionRequest,
        };
        // The creator's BYOK key presence for the managed-LLM profile
        // closure (D-11 `drive_transition`).
        let key_row: Option<Option<Vec<u8>>> = assistant_user_llm_config::Entity::find()
            .select_only()
            .column(assistant_user_llm_config::Column::ApiKeyEncrypted)
            .filter(assistant_user_llm_config::Column::UserId.eq(*actor_id))
            .into_tuple()
            .one(&pidash_db::pool::sea_orm_connection(&self.pool))
            .await
            .map_err(orm_err)?;
        let has_key = key_row.flatten().is_some_and(|key| !key.is_empty());
        let actor = *actor_id;
        let cloud = self.cloud.clone();
        let managed = self.managed.clone();
        let toolsets = Arc::clone(&self.toolsets);
        let now_unix_secs = now.timestamp();
        // Built twice (store + preflight take owned deps; the driver
        // consults preflight at most once per drive).
        let make_deps = || StoreDeps {
            cloud: cloud.clone(),
            managed: managed.clone(),
            cache: RedisAdmissionCache::from_env(),
            toolsets: Arc::clone(&toolsets),
            has_usable_llm_config: move |creator: Uuid| creator == actor && has_key,
            llm_profile: move |user: Option<Uuid>| {
                let key = user == Some(actor) && has_key;
                let profile = pidash_services::assistant::seams::agent_model_profile_for_user(key);
                pidash_services::dispatch::LlmProfile {
                    available: profile.available,
                    reason_code: profile.reason_code,
                }
            },
            now_unix_secs,
        };
        // `jitter_secs: 0.0`: the L5 quirk-5 precedent (the v1 human-move
        // drivers pass 0.0 as well). `moved_by_run: None`: the tools are
        // human-equivalent moves, so the entry-run builder is reachable.
        let request = TransitionRequest {
            issue_id: *issue_id,
            from_state,
            to_state: Some(to_state),
            actor: Some(*actor_id),
            dispatch_immediate: true,
            moved_by_run: None,
            now: *now,
            jitter_secs: 0.0,
            created_by,
            pod_once,
        };
        // One transaction for the whole drive. This entry serves the
        // `dispatch_coding_run` path only: Python has no outer atomic
        // there (`runs.py:37-96`), so the drive owns its transaction
        // and no row lock is held while the pool-side preflight runs.
        // The `update_issue` path fuses its drive into the save
        // transaction instead (see `update_issue_row`,
        // PIDASHCONV-1124).
        let tx = pidash_db::tx::Transaction::begin(&self.pool).await?;
        let mut store = LiveCreationStore::new(tx, &self.pool, make_deps());
        let mut preflight = TurnPreflight {
            pool: self.pool.clone(),
            deps: Some(make_deps()),
            matcher: crate::v1_work_items::handlers_actions::LiveMatcher::new(self.pool.clone()),
            now: *now,
        };
        let outcome = handle_issue_state_transition(&mut store, &mut preflight, &request).await;
        let (tx, pool, cache, dispatches, deferred, terminal_effects) = store.into_parts();
        let outcome = outcome.map_err(|error| match error {
            pidash_services::orchestration::entries::EntriesError::Store(store) => {
                StoreError::Transition(store.to_string())
            }
            other => StoreError::Transition(other.to_string()),
        })?;
        tx.commit().await?;
        // A finalized run is a normal outcome, not an error: the
        // prompt-build failure path (`service.py:820-832`) finalizes
        // the run FAILED and returns `render-failed` with no
        // exception — Django commits and the tool returns its outcome
        // normally. Drain each finalized run's publish pair the
        // jobs-context way (the `oss_worker` precedent,
        // PIDASHCONV-859): enqueue the terminal-effects task
        // best-effort; the inline apply is skipped, the queued task
        // covers the hooks idempotently (PIDASHCONV-1571).
        for run_id in &terminal_effects {
            drain_finalized_run(pool, *run_id).await;
        }
        // Post-commit dispatch drain: unlike D-11 (whose `moved_by_run`
        // makes the outbox unreachable), human moves dispatch here —
        // after the commit, so the runner read sees the rows. A
        // dispatch failure propagates: Django dispatches from
        // `on_commit`, outside the signal `try`, so its tool fails
        // there too (the save stands either way).
        for run_id in dispatches {
            pidash_jobs::dispatch::dispatch_agent_run(pool, &self.cloud, run_id, *now)
                .await
                .map_err(StoreError::from)?;
        }
        for consume in &deferred {
            pidash_services::dispatch::admission::consume_admission_token(&cache, consume);
        }
        Ok(TransitionOutcome {
            created_run_id: outcome.created_run,
            reason: outcome.reason,
        })
    }

    async fn drive_create_transition(
        &self,
        issue_id: &Uuid,
        to_state: pidash_services::orchestration::creation::StateView,
        actor_id: &Uuid,
        now: &DateTime<Utc>,
    ) -> Result<Option<TransitionOutcome>, StoreError> {
        use pidash_jobs::admission::RedisAdmissionCache;
        use pidash_jobs::orchestration::{LiveCreationStore, StoreDeps};
        use pidash_services::orchestration::entries::{
            fire_state_transition, FireOutcome, FireRequest,
        };
        // The creator's BYOK key presence for the managed-LLM profile
        // closure (D-11 `drive_transition`).
        let key_row: Option<Option<Vec<u8>>> = assistant_user_llm_config::Entity::find()
            .select_only()
            .column(assistant_user_llm_config::Column::ApiKeyEncrypted)
            .filter(assistant_user_llm_config::Column::UserId.eq(*actor_id))
            .into_tuple()
            .one(&pidash_db::pool::sea_orm_connection(&self.pool))
            .await
            .map_err(orm_err)?;
        let has_key = key_row.flatten().is_some_and(|key| !key.is_empty());
        let actor = *actor_id;
        let cloud = self.cloud.clone();
        let managed = self.managed.clone();
        let toolsets = Arc::clone(&self.toolsets);
        let now_unix_secs = now.timestamp();
        // Built twice (store + preflight take owned deps; the driver
        // consults preflight at most once per drive).
        let make_deps = || StoreDeps {
            cloud: cloud.clone(),
            managed: managed.clone(),
            cache: RedisAdmissionCache::from_env(),
            toolsets: Arc::clone(&toolsets),
            has_usable_llm_config: move |creator: Uuid| creator == actor && has_key,
            llm_profile: move |user: Option<Uuid>| {
                let key = user == Some(actor) && has_key;
                let profile = pidash_services::assistant::seams::agent_model_profile_for_user(key);
                pidash_services::dispatch::LlmProfile {
                    available: profile.available,
                    reason_code: profile.reason_code,
                }
            },
            now_unix_secs,
        };
        // `fire_state_transition` (`signals.py:74-108`): prior state
        // None (a create has no pre-save row), the saved state, the
        // receiver defaults. `jitter_secs: 0.0`: the L5 quirk-5
        // precedent (the v1 human-move drivers pass 0.0 as well).
        // `moved_by_run: None`: the tools are human-equivalent moves,
        // so the entry-run builder is reachable. `created_by:
        // Some(actor)`: the create path runs impersonated
        // (`issues.py:256`).
        let request = FireRequest {
            issue_id: *issue_id,
            prev_state_id: None,
            current_state_id: Some(to_state.id),
            dispatch_immediate: true,
            pod_once: false,

            moved_by_run: None,
            now: *now,
            jitter_secs: 0.0,
            created_by: Some(*actor_id),
        };
        // One transaction for the whole drive: the create already
        // committed (no outer atomic survives `create_issue_row`), so
        // the drive owns it. Preflight runs on that same transaction
        // — the `update_issue_row` fused shape (PIDASHCONV-1124): the
        // bounce must see this drive's own uncommitted ticker row
        // (the explicit-disarm arm) — a pool-side bounce (as
        // `TurnPreflight` runs on the dispatch path) would see no row
        // and skip the disarm, while a pool-side lock would hang on
        // this transaction's ticker lock. So the driver lends the tx
        // to only one seam at a time: probe the decision behind a
        // savepoint (the probe's bounce writes roll back), drive with
        // a fixed-decision stub, and — only when stubbed false —
        // re-run the services preflight after the drive, so the real
        // bounce lands post-reconcile, exactly as in Python.
        let matcher = crate::v1_work_items::handlers_actions::LiveMatcher::new(self.pool.clone());
        let triggered_by = pidash_db::dispatch::status::AgentRunTrigger::StateTransition.value();
        let mut tx = pidash_db::tx::Transaction::begin(&self.pool).await?;
        self.probe_savepoint(&mut tx).await?;
        let mut probe_store = LiveCreationStore::new(tx, &self.pool, make_deps());
        // `resolve_pod_for_issue` reads issue + pods only (no
        // ticker), so probing pre-reconcile decides identically to
        // Python's post-reconcile preflight. `None` still stubs
        // false: the drive re-resolves the pod itself, and only
        // consults the stub when it finds one (the appear-race) —
        // the re-run then derives the true decision.
        let probed_pod = pidash_services::orchestration::creation::resolve_pod_for_issue(
            &mut probe_store,
            *issue_id,
        )
        .await
        .map_err(|error| StoreError::Transition(error.to_string()))?;
        let proceed = match probed_pod {
            Some(pod_id) => {
                let probe =
                    pidash_services::orchestration::dispatch::preflight_eligibility_or_bounce(
                        &mut probe_store,
                        *issue_id,
                        Some(actor),
                        pod_id,
                        triggered_by,
                        &matcher,
                        *now,
                        0.0,
                    )
                    .await
                    .map_err(|error| StoreError::Transition(error.to_string()))?;
                probe.proceed
            }
            None => false,
        };
        // The probe's outboxes stay empty (the bounce dispatches,
        // defers, and finalizes nothing), so only the tx is
        // recovered; the savepoint rollback erases the probe's
        // writes, and its log lines are dropped (the re-run emits
        // the real bounce's lines once).
        let (mut tx, _, _, _, _, _) = probe_store.into_parts();
        self.rollback_probe_savepoint(&mut tx).await?;
        let mut drive_store = LiveCreationStore::new(tx, &self.pool, make_deps());
        let mut stub = FixedPreflight {
            proceed,
            consulted: false,
        };
        let outcome = fire_state_transition(&mut drive_store, &mut stub, &request).await;
        let (tx, pool, cache, mut dispatches, mut deferred, mut terminal_effects) =
            drive_store.into_parts();
        // Only a state-*lookup* storage failure propagates (Python's
        // `try` wraps the handler call alone, `signals.py:89-98`); a
        // handler failure arrives as `Failed` and is swallowed below.
        let outcome = outcome.map_err(|error| match error {
            pidash_services::orchestration::entries::EntriesError::Store(store) => {
                StoreError::Transition(store.to_string())
            }
            other => StoreError::Transition(other.to_string()),
        })?;
        // Only a consulted stub owes the re-run: early returns
        // (parked, queued, no creator/pod, race lost) consult
        // nothing, and Python bounces on none of them.
        let (tx, dispatches, deferred, terminal_effects) = if !proceed && stub.consulted {
            // The re-run IS the real bounce when it still says so
            // (same services fn, same tx, post-reconcile state);
            // when eligibility flipped concurrently it proceeds and
            // the armed ticker covers the entry on the next tick.
            // Fresh pod first (closest to Python's read position),
            // the probed id when the pod vanished mid-drive (as
            // Python's single read would have seen it).
            let mut bounce_store = LiveCreationStore::new(tx, &self.pool, make_deps());
            let fresh_pod = pidash_services::orchestration::creation::resolve_pod_for_issue(
                &mut bounce_store,
                *issue_id,
            )
            .await
            .map_err(|error| StoreError::Transition(error.to_string()))?;
            if let Some(pod_id) = fresh_pod.or(probed_pod) {
                let rerun =
                    pidash_services::orchestration::dispatch::preflight_eligibility_or_bounce(
                        &mut bounce_store,
                        *issue_id,
                        Some(actor),
                        pod_id,
                        triggered_by,
                        &matcher,
                        *now,
                        0.0,
                    )
                    .await
                    .map_err(|error| StoreError::Transition(error.to_string()))?;
                for log in &rerun.logs {
                    if log.level == "WARNING" {
                        tracing::warn!(target: "pidash.preflight", "{}", log.message);
                    } else {
                        tracing::info!(target: "pidash.preflight", "{}", log.message);
                    }
                }
            } else {
                tracing::warn!(
                    target: "pidash.preflight",
                    "turn create drive: pod vanished mid-drive for issue {issue_id}, skipping bounce",
                );
            }
            let (tx, _, _, redispatches, redeferred, reterminal) = bounce_store.into_parts();
            dispatches.extend(redispatches);
            deferred.extend(redeferred);
            terminal_effects.extend(reterminal);
            (tx, dispatches, deferred, terminal_effects)
        } else {
            (tx, dispatches, deferred, terminal_effects)
        };
        match outcome {
            FireOutcome::NoTransition => {
                let _ = tx.rollback().await;
                Ok(None)
            }
            FireOutcome::Failed { log_line, .. } => {
                // The save stands; nothing this drive wrote is kept
                // (the `fire_issue_transition` precedent,
                // PIDASHCONV-1216). The counter already bumped inside.
                let _ = tx.rollback().await;
                tracing::error!("{log_line}");
                Ok(None)
            }
            FireOutcome::Called(transition) => {
                tx.commit().await?;
                // A finalized run is a normal outcome, not an error:
                // the prompt-build failure path (`service.py:820-832`)
                // finalizes the run FAILED and returns `render-failed`
                // with no exception — Django commits and the tool
                // briefs normally. Drain each finalized run's publish
                // pair the jobs-context way (the `oss_worker`
                // precedent, PIDASHCONV-859): enqueue the
                // terminal-effects task best-effort; the inline apply
                // is skipped, the queued task covers the hooks
                // idempotently (PIDASHCONV-1571).
                for run_id in &terminal_effects {
                    drain_finalized_run(pool, *run_id).await;
                }
                // Post-commit dispatch drain: human moves dispatch
                // here — after the commit, so the runner read sees
                // the rows (the `drive_transition` drain shape). A
                // dispatch failure propagates: Django dispatches
                // from `on_commit`, outside the signal `try`, so its
                // tool fails there too (the save stands either way).
                for run_id in dispatches {
                    pidash_jobs::dispatch::dispatch_agent_run(pool, &self.cloud, run_id, *now)
                        .await
                        .map_err(StoreError::from)?;
                }
                for consume in &deferred {
                    pidash_services::dispatch::admission::consume_admission_token(&cache, consume);
                }
                Ok(Some(TransitionOutcome {
                    created_run_id: transition.created_run,
                    reason: transition.reason,
                }))
            }
        }
    }

    async fn create_comment_row(
        &self,
        input: &NewCommentInput,
        now: &DateTime<Utc>,
    ) -> Result<Uuid, StoreError> {
        // `comments.create_comment :53-65` + `IssueComment.save`: the
        // comment INSERT (MLStripper `comment_stripped`, Django
        // `strip_tags` `Description` row — NULL for empty HTML), the
        // `Description` INSERT, the `description_id` link. One
        // transaction; FKs are deferred, so a TOCTOU violation fires at
        // commit → internal, as the propagated `IntegrityError` does.
        let comment_id = Uuid::new_v4();
        let stripped = if input.html.is_empty() {
            String::new()
        } else {
            pidash_db::app_pages::strip::ml_strip_tags(&input.html)
        };
        let desc_stripped: Option<String> = if input.html.is_empty() {
            None
        } else {
            Some(crate::space::sanitize::strip_tags(&input.html))
        };
        let labels: Vec<String> = if input.fold {
            vec!["fold".to_owned()]
        } else {
            Vec::new()
        };
        // Domain twins, in Django order; one `now()` feeds all four
        // stamps. The twin binds `attachments` as a typed empty array
        // (`'{}'`): the retired statement bound `Value::Array([])`,
        // which sqlx sends as jsonb, so the INSERT failed on every
        // call (PIDASHCONV-1582) — this migration is error-to-success.
        let empty_json = Value::Object(serde_json::Map::new());
        let empty_list: Vec<String> = Vec::new();
        let rows = comment_domain::CommentRows {
            id: comment_id,
            description_id: Uuid::new_v4(),
            created_at: *now,
            updated_at: *now,
            desc_created_at: *now,
            desc_updated_at: *now,
            workspace_id: input.workspace_id,
            project_id: input.project_id,
            issue_id: input.issue_id,
            actor_id: Some(input.actor_id),
            created_by: Some(input.actor_id),
            updated_by: None,
            deleted_at: None,
            html: &input.html,
            json: &empty_json,
            stripped: &stripped,
            desc_stripped: desc_stripped.as_deref(),
            access: "INTERNAL",
            speaker_type: "agent",
            speaker_label: "Pi Dash AI",
            speaker_agent_run_id: None,
            external_id: None,
            external_source: None,
            parent_id: None,
            attachments: &empty_list,
            labels: &labels,
            edited_at: None,
        };
        let mut tx = self.pool.begin().await?;
        comment_domain::insert_comment_row(&pidash_db::orm_tx::TxConn::new(&mut tx), &rows).await?;
        comment_domain::insert_description_row(&pidash_db::orm_tx::TxConn::new(&mut tx), &rows)
            .await?;
        comment_domain::link_comment_description(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            rows.id,
            rows.description_id,
        )
        .await?;
        tx.commit().await?;
        Ok(comment_id)
    }

    async fn record_write(
        &self,
        deps: &AssistantDeps,
        summary: &str,
        links: &[Value],
        now: &DateTime<Utc>,
    ) -> Result<(), StoreError> {
        use pidash_db::assistant::event_queries as events;
        // The open turn, if any (`filter(pk).first()` — None is fine).
        // The payload below still renders `str(deps.turn_id)`
        // unconditionally (`_results.py:107`).
        let turn: Option<Uuid> = assistant_turn::Entity::find()
            .select_only()
            .column(assistant_turn::Column::Id)
            .filter(assistant_turn::Column::Id.eq(deps.turn_id))
            .into_tuple()
            .one(&pidash_db::pool::sea_orm_connection(&self.pool))
            .await
            .map_err(orm_err)?;
        // Message row (`events.create_message`): own transaction, thread
        // locked, thread-scoped seq. Missing thread is `DoesNotExist` →
        // internal. The lock blocks (as Python's `SELECT ... FOR
        // UPDATE` does) rather than using the builder's `SKIP LOCKED`,
        // whose rationale is the turn hot path, not this post-write
        // fan-out; the single-lock scope cannot deadlock.
        let message_id = Uuid::new_v4();
        let mut links_payload = serde_json::Map::with_capacity(1);
        links_payload.insert("links".to_owned(), Value::Array(links.to_vec()));
        let message_payload = Value::Object(links_payload);
        let message_seq: i64;
        {
            let mut tx = self.pool.begin().await?;
            let locked: Option<Uuid> = assistant_thread::Entity::find_by_id(deps.thread_id)
                .lock_exclusive()
                .select_only()
                .column(assistant_thread::Column::Id)
                .into_tuple()
                .one(&pidash_db::orm_tx::TxConn::new(&mut tx))
                .await
                .map_err(orm_err)?;
            if locked.is_none() {
                return Err(StoreError::Missing {
                    model: "AssistantThread",
                });
            }
            let max: Option<Option<i64>> = assistant_message::Entity::find()
                .select_only()
                .column_as(assistant_message::Column::Seq.max(), "m")
                .filter(assistant_message::Column::ThreadId.eq(deps.thread_id))
                .into_tuple()
                .one(&pidash_db::orm_tx::TxConn::new(&mut tx))
                .await
                .map_err(orm_err)?;
            message_seq = max.flatten().unwrap_or(0) + 1;
            pidash_services::domain::assistant::create_message_row(
                &pidash_db::orm_tx::TxConn::new(&mut tx),
                &pidash_services::domain::assistant::NewMessage {
                    id: message_id,
                    thread_id: deps.thread_id,
                    turn_id: turn,
                    seq: message_seq,
                    kind: pidash_services::assistant::tools_results::TOOL_RESULT_KIND,
                    display_content: summary,
                    payload: &message_payload,
                    status: "completed",
                    created_at: *now,
                    completed_at: None,
                },
            )
            .await?;
            tx.commit().await?;
        }
        // Event row (`events.append_event`): own transaction, thread
        // locked, thread-scoped seq; the payload embeds the envelope.
        let created_at = crate::runner_enroll::enroll::render_isoformat(*now);
        let envelope = message_envelope(
            &message_id,
            summary,
            message_seq,
            turn,
            message_payload,
            &created_at,
        );
        let mut event_payload = serde_json::Map::with_capacity(2);
        event_payload.insert(
            "turn_id".to_owned(),
            Value::String(deps.turn_id.to_string()),
        );
        event_payload.insert("message".to_owned(), envelope);
        let event_payload = Value::Object(event_payload);
        let event_seq: i64;
        {
            let mut tx = self.pool.begin().await?;
            let locked: Option<Uuid> = assistant_thread::Entity::find_by_id(deps.thread_id)
                .lock_exclusive()
                .select_only()
                .column(assistant_thread::Column::Id)
                .into_tuple()
                .one(&pidash_db::orm_tx::TxConn::new(&mut tx))
                .await
                .map_err(orm_err)?;
            if locked.is_none() {
                return Err(StoreError::Missing {
                    model: "AssistantThread",
                });
            }
            let max: Option<Option<i64>> = assistant_event::Entity::find()
                .select_only()
                .column_as(assistant_event::Column::Seq.max(), "m")
                .filter(assistant_event::Column::ThreadId.eq(deps.thread_id))
                .into_tuple()
                .one(&pidash_db::orm_tx::TxConn::new(&mut tx))
                .await
                .map_err(orm_err)?;
            event_seq = max.flatten().unwrap_or(0) + 1;
            pidash_services::domain::assistant::append_event(
                &pidash_db::orm_tx::TxConn::new(&mut tx),
                &pidash_services::domain::assistant::NewEvent {
                    thread_id: deps.thread_id,
                    turn_id: turn,
                    seq: event_seq,
                    kind: pidash_services::assistant::tools_results::TOOL_RESULT_KIND,
                    message_id: Some(message_id),
                    payload: &event_payload,
                    created_at: *now,
                },
            )
            .await?;
            tx.commit().await?;
        }
        // Best-effort publish (`events.publish_event` swallows every
        // failure, logging and returning). The event id for the frame
        // is re-read (the builder has no `RETURNING`); the payload
        // renders spaced (`json.dumps`), not compact.
        if let Some(redis) = &self.redis {
            let event_id: Option<i64> = assistant_event::Entity::find()
                .select_only()
                .column(assistant_event::Column::Id)
                .filter(assistant_event::Column::ThreadId.eq(deps.thread_id))
                .filter(assistant_event::Column::Seq.eq(event_seq))
                .into_tuple()
                .one(&pidash_db::pool::sea_orm_connection(&self.pool))
                .await
                .map_err(orm_err)?;
            if let Some(event_id) = event_id {
                let thread_id = deps.thread_id.to_string();
                let message_id = message_id.to_string();
                let payload_json =
                    pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&event_payload);
                let frame = events::serialize_event_json(&events::EventParts {
                    id: event_id,
                    thread_id: &thread_id,
                    message_id: Some(&message_id),
                    seq: event_seq,
                    kind: pidash_services::assistant::tools_results::TOOL_RESULT_KIND,
                    payload_json: &payload_json,
                    created_at: &created_at,
                });
                let (channel, body) = events::publish_frame(&thread_id, &frame);
                let publish = async {
                    use redis::AsyncCommands;
                    let mut connection = redis.get_multiplexed_async_connection().await?;
                    connection.publish::<_, _, ()>(&channel, &body).await
                };
                if let Err(error) = publish.await {
                    tracing::warn!(%error, "assistant publish failed; write stands");
                }
            }
        }
        Ok(())
    }

    async fn github_binding_config(
        &self,
        deps: &AssistantDeps,
        provider: &str,
        namespace: &str,
        repo: &str,
    ) -> Result<Option<Value>, StoreError> {
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let config: Option<Value> = Self::binding_config_query(deps, provider, namespace, repo)
            .select_only()
            .column(git_provider_accounts::Column::CredentialConfig)
            .into_tuple()
            .one(&db)
            .await
            .map_err(orm_err)?;
        Ok(config)
    }

    async fn github_sync_credentials(
        &self,
        deps: &AssistantDeps,
        namespace: &str,
        repo: &str,
    ) -> Result<Option<Value>, StoreError> {
        let db = pidash_db::pool::sea_orm_connection(&self.pool);
        let member_projects = projects::Entity::find()
            .join(
                JoinType::InnerJoin,
                projects::Relation::ProjectMembers.def(),
            )
            .join(JoinType::InnerJoin, projects::Relation::Workspaces.def())
            .select_only()
            .column(projects::Column::Id)
            .distinct()
            .filter(project_members::Column::MemberId.eq(deps.user_id))
            .filter(project_members::Column::IsActive.eq(true))
            .filter(workspaces::Column::Slug.eq(&deps.workspace_slug))
            .filter(projects::Column::DeletedAt.is_null())
            .into_query();
        let credentials: Option<Value> = github_repository_syncs::Entity::find()
            .join(
                JoinType::InnerJoin,
                github_repository_syncs::Relation::GithubRepositories.def(),
            )
            .select_only()
            .column(github_repository_syncs::Column::Credentials)
            .filter(github_repository_syncs::Column::ProjectId.in_subquery(member_projects))
            .filter(
                Expr::expr(Func::upper(Expr::col((
                    github_repositories::Entity,
                    github_repositories::Column::Owner,
                ))))
                .eq(Func::upper(Expr::val(namespace.to_owned()))),
            )
            .filter(
                Expr::expr(Func::upper(Expr::col((
                    github_repositories::Entity,
                    github_repositories::Column::Name,
                ))))
                .eq(Func::upper(Expr::val(repo.to_owned()))),
            )
            .filter(github_repository_syncs::Column::DeletedAt.is_null())
            .order_by_desc(github_repository_syncs::Column::CreatedAt)
            .into_tuple()
            .one(&db)
            .await
            .map_err(orm_err)?;
        Ok(credentials)
    }

    fn decrypt_token(&self, stored: Option<&str>) -> Option<String> {
        use pidash_services::assistant::tools_github::maybe_decrypt;
        // `github._maybe_decrypt` over the real `decrypt_data` (which
        // catches everything and answers `""`, so the `None`-outcome
        // arm is unreachable — `Some` always).
        match stored {
            None => None,
            Some(token) => {
                let outcome =
                    pidash_services::license::encryption::decrypt_data(&self.keyring, Some(token));
                maybe_decrypt(Some(token), Some(&outcome))
            }
        }
    }

    async fn fetch_pr(&self, api_url: &str, headers: &[(String, String)]) -> PrFetch {
        // `github.py:125-127`: GET, 10s timeout (client-level), no
        // redirects (client-level). Any transport failure is
        // `NetworkError` (→ `unknown`/`network_error`, never raised).
        let mut request = self.http.get(api_url);
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(_) => return PrFetch::NetworkError,
        };
        let status = response.status().as_u16();
        // A body-read failure is a transport failure (`network_error`,
        // as the read happens inside `client.get`); a parse failure is
        // `bad_response` (`github.py:147-149`).
        let bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(_) => return PrFetch::NetworkError,
        };
        let body: Option<Value> = serde_json::from_slice(&bytes).ok();
        PrFetch::Fetched { status, body }
    }
}

/// Fixed-decision preflight for the fused update drive: the decision
/// was probed on the fused transaction behind a savepoint (see
/// `update_issue_row`), and the driver consults this at most once, in
/// exactly the position Python consults
/// `scheduling.preflight_eligibility_or_bounce`. `consulted` tells the
/// fused drive whether the re-run owes a real bounce (early returns
/// never consult, and Python bounces on none of them).
struct FixedPreflight {
    proceed: bool,
    consulted: bool,
}

impl pidash_services::orchestration::entries::PreflightSeam for FixedPreflight {
    async fn preflight_eligibility_or_bounce(
        &mut self,
        _issue_id: Uuid,
        _creator_id: Uuid,
        _pod_id: Uuid,
        _triggered_by: &str,
    ) -> Result<bool, pidash_services::orchestration::creation::CreationError> {
        self.consulted = true;
        Ok(self.proceed)
    }

    async fn pod_once_preflight(
        &mut self,
        _issue_id: Uuid,
        _creator_id: Uuid,
        _pod_id: Uuid,
    ) -> Result<Option<String>, pidash_services::orchestration::creation::CreationError> {
        Ok(None)
    }
}

/// The human-move preflight (the `v1_work_items` `CorePreflight`
/// shape): the jobs eligibility driver in its own transaction and
/// commit — like Django's bounce atomic — over the actions
/// `LiveMatcher`, logs emitted, `proceed` returned. Serves the
/// `dispatch_coding_run` drive only (no row lock is held there, so
/// the pool-side bounce never blocks). The deps travel by `Option`
/// because the driver takes them by value while the seam only lends
/// `&mut self`; the driver consults preflight at most once per
/// drive, so the `None` arm is defensive.
struct TurnPreflight<C, H, P> {
    pool: PgPool,
    deps: Option<pidash_jobs::orchestration::StoreDeps<C, H, P>>,
    matcher: crate::v1_work_items::handlers_actions::LiveMatcher,
    now: DateTime<Utc>,
}

impl<C, H, P> pidash_services::orchestration::entries::PreflightSeam for TurnPreflight<C, H, P>
where
    C: pidash_services::dispatch::admission::AdmissionCache,
    H: Fn(Uuid) -> bool + Clone + Send + Sync,
    P: Fn(Option<Uuid>) -> pidash_services::dispatch::LlmProfile + Clone + Send + Sync,
{
    async fn preflight_eligibility_or_bounce(
        &mut self,
        issue_id: Uuid,
        creator_id: Uuid,
        pod_id: Uuid,
        triggered_by: &str,
    ) -> Result<bool, pidash_services::orchestration::creation::CreationError> {
        use pidash_services::orchestration::creation::CreationError;
        let Some(deps) = self.deps.take() else {
            return Err(CreationError::Db(
                "turn preflight deps already consumed".to_owned(),
            ));
        };
        let out = pidash_jobs::orchestration::preflight_eligibility_or_bounce(
            &self.pool,
            deps,
            &self.matcher,
            self.now,
            0.0,
            issue_id,
            Some(creator_id),
            pod_id,
            triggered_by,
        )
        .await
        .map_err(|error| CreationError::Db(error.to_string()))?;
        for log in &out.outcome.logs {
            if log.level == "WARNING" {
                tracing::warn!(target: "pidash.preflight", "{}", log.message);
            } else {
                tracing::info!(target: "pidash.preflight", "{}", log.message);
            }
        }
        Ok(out.outcome.proceed)
    }

    async fn pod_once_preflight(
        &mut self,
        issue_id: Uuid,
        creator_id: Uuid,
        pod_id: Uuid,
    ) -> Result<Option<String>, pidash_services::orchestration::creation::CreationError> {
        pod_once_preflight_impl(&self.pool, &self.matcher, issue_id, creator_id, pod_id).await
    }
}

/// The live `pod_once` preflight (PDASHOSS01-350, design §23.3): the
/// domain authorization plus the pod-branch availability over the
/// tool execution seams. Shared by the turn and tool engines so the
/// two preflight wrappers cannot drift.
pub(crate) async fn pod_once_preflight_impl<
    M: pidash_services::orchestration::dispatch::PodRunnerMatcher,
>(
    pool: &PgPool,
    matcher: &M,
    issue_id: Uuid,
    creator_id: Uuid,
    pod_id: Uuid,
) -> Result<Option<String>, pidash_services::orchestration::creation::CreationError> {
    use pidash_services::orchestration::creation::CreationError;
    use sea_orm::EntityTrait;
    let conn = pidash_db::pool::sea_orm_connection(pool);
    let issue = pidash_db::entity::issues::Entity::find_by_id(issue_id)
        .one(&conn)
        .await
        .map_err(|error| CreationError::Db(error.to_string()))?
        .ok_or_else(|| CreationError::Db("issue not found".to_owned()))?;
    if let Some(code) = pidash_services::domain::issue_assignment::pod_once_auth_code(
        &conn,
        &issue,
        Some(creator_id),
    )
    .await
    .map_err(|error| CreationError::Db(error.to_string()))?
    {
        return Ok(Some(code.to_owned()));
    }
    let has = matcher
        .pod_has_runner_for_issue_principal(pod_id, issue_id, Some(creator_id), true, true)
        .await
        .map_err(|error| CreationError::Db(error.to_string()))?;
    Ok(if has {
        None
    } else {
        Some("no_eligible_runner_in_pod".to_owned())
    })
}

/// Drain one finalized run's publish pair from a turn drive
/// (`agent_run_finalization.py:89-103`, the `oss_worker` jobs-context
/// shape): enqueue the terminal-effects task best-effort — the emit's
/// own `try` in Python — and skip the inline apply, which needs live
/// runner seams the turn store does not hold. The queued task covers
/// the hooks idempotently (PIDASHCONV-859); the reconciler closes any
/// remaining window. Never fails the drive (PIDASHCONV-1571).
async fn drain_finalized_run(pool: &PgPool, run_id: Uuid) {
    use pidash_services::runner_runs::LifecycleEffect;
    for effect in pidash_services::runner_runs::finalization::plan_publish_effects(run_id) {
        match effect {
            LifecycleEffect::PublishTerminalEffects { run_id } => {
                let job = pidash_jobs::runner_runs::sweeps_chat::terminal_effects_job(&run_id);
                if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
                    tracing::error!(%error, "failed to publish terminal effects for run {run_id}");
                }
            }
            LifecycleEffect::ApplyTerminalEffectsInline { run_id } => {
                tracing::warn!(
                    "skipping inline terminal effects for run {run_id}: no live RunnerRunsSeams \
                     (PIDASHCONV-859); the queued effects task covers the hooks idempotently"
                );
            }
            // Unreachable: `plan_publish_effects` emits exactly these two.
            _ => {}
        }
    }
}

/// The `message_envelope` wire shape (`events.message_envelope`,
/// `assistant/runtime/events.py:128-140`): `kind` → `role`
/// (`tool_result` on this path), `turn_id` null when the turn row is
/// gone. Key order is parse-equal only.
fn message_envelope(
    message_id: &Uuid,
    summary: &str,
    message_seq: i64,
    turn: Option<Uuid>,
    payload: Value,
    created_at: &str,
) -> Value {
    let mut envelope = serde_json::Map::with_capacity(9);
    envelope.insert("id".to_owned(), Value::String(message_id.to_string()));
    envelope.insert(
        "role".to_owned(),
        Value::String(pidash_services::assistant::tools_results::TOOL_RESULT_KIND.to_owned()),
    );
    envelope.insert("content".to_owned(), Value::String(summary.to_owned()));
    envelope.insert("status".to_owned(), Value::String("completed".to_owned()));
    envelope.insert("seq".to_owned(), Value::from(message_seq));
    envelope.insert(
        "turn_id".to_owned(),
        turn.map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    envelope.insert("payload".to_owned(), payload);
    envelope.insert(
        "created_at".to_owned(),
        Value::String(created_at.to_owned()),
    );
    envelope.insert("completed_at".to_owned(), Value::Null);
    Value::Object(envelope)
}

/// Best-effort post-write task fan-out for the `.delay()` call
/// sites: without a queue row the op still stands (the
/// `v1_work_items` `enqueue_best_effort` precedent — a deliberate
/// divergence from `.delay()`, which propagates broker failures).
async fn enqueue_best_effort(
    pool: &PgPool,
    task: &str,
    args: Vec<Value>,
    kwargs: serde_json::Map<String, Value>,
) {
    let message = pidash_jobs::TaskMessage::new(task, args, kwargs);
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task, "task enqueue failed; op stands");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{DbBackend, QueryTrait};

    /// The identifier sequence predicate keeps the DB-side `::int` cast
    /// (out-of-int4 tails raise; a Rust `as i32` would wrap 4294967297
    /// to 1 and look up the wrong issue).
    #[test]
    fn sequence_filter_keeps_db_side_int_cast() {
        let sql = issues::Entity::find()
            .filter(sequence_filter(4294967297))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            sql.contains("CAST(4294967297 AS int)"),
            "sequence predicate must cast in the DB, got: {sql}"
        );
    }

    /// The issue row lock is single-table `FOR UPDATE` (a join-lock over
    /// the `issues LEFT JOIN states` read fails at runtime: "cannot be
    /// applied to the nullable side of an outer join").
    #[test]
    fn locked_issue_query_locks_one_table() {
        let sql = locked_issue_query(Uuid::from_u128(7))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("FOR UPDATE"), "lock missing: {sql}");
        assert!(!sql.contains("JOIN"), "lock must not join: {sql}");
    }

    /// The event envelope carries `role: "tool_result"` (`kind` →
    /// `role`, `events.message_envelope`), with the full 9-key shape
    /// (`runtime/events.py:128-140`, parse-equal: key order differs).
    #[test]
    fn message_envelope_reports_tool_result_role() {
        let message_id = Uuid::from_u128(11);
        let turn_id = Uuid::from_u128(12);
        let out = message_envelope(
            &message_id,
            "Did the thing",
            3,
            Some(turn_id),
            serde_json::json!({"links": []}),
            "2026-03-13T12:00:00+00:00",
        );
        assert_eq!(
            out,
            serde_json::json!({
                "id": message_id.to_string(),
                "role": "tool_result",
                "content": "Did the thing",
                "status": "completed",
                "seq": 3,
                "turn_id": turn_id.to_string(),
                "payload": {"links": []},
                "created_at": "2026-03-13T12:00:00+00:00",
                "completed_at": null,
            })
        );
        // A gone turn row renders `turn_id: null` (the filter-first
        // still renders `str(deps.turn_id)` in the outer payload).
        let out = message_envelope(
            &message_id,
            "Did the thing",
            3,
            None,
            serde_json::json!({"links": []}),
            "2026-03-13T12:00:00+00:00",
        );
        assert_eq!(out["turn_id"], serde_json::Value::Null);
    }

    async fn live_pool() -> PgPool {
        let url = std::env::var("DATABASE_URL").expect("export DATABASE_URL for live turn tests");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .expect("connect scratch database");
        let db: String = sqlx::query_scalar("SELECT current_database()")
            .fetch_one(&pool)
            .await
            .expect("current database");
        assert_ne!(db, "postgres", "refusing the shared postgres database");
        pool
    }

    fn live_cloud_settings() -> pidash_db::config::CloudAgentSettings {
        pidash_db::config::CloudAgentSettings {
            enabled: false,
            writes_enabled: false,
            github_tools_enabled: true,
            disabled_tools: Vec::new(),
            job_queue: "celery".to_string(),
            reconcile_interval_secs: 30,
            model_request_timeout_secs: 60,
            execution_timeout_secs: 285,
            run_soft_limit_secs: 300,
            run_hard_limit_secs: 330,
            stale_grace_secs: 60,
            dispatch_lease_secs: 60,
            dispatch_backoff_secs: 10,
            dispatch_scan_interval_secs: 10,
            sweep_interval_secs: 30,
            dispatch_scan_batch: 100,
            max_queue_age_secs: 900,
            model_request_limit: 25,
            tool_call_limit: 20,
            write_call_limit: 3,
            input_token_limit: 144_000,
            output_token_limit: 16_000,
            total_token_limit: 160_000,
            max_output_tokens_per_request: 4096,
            max_queued_per_workspace: 20,
            max_running_per_workspace: 2,
            user_creation_rate_per_minute: 6,
            workspace_creation_rate_per_minute: 30,
            tool_timeout_secs: 20,
            max_tool_result_bytes: 65536,
            max_prompt_bytes: 262_144,
            max_final_result_bytes: 65536,
            max_events: 500,
            block_private_urls: true,
        }
    }

    fn live_managed_settings() -> pidash_db::config::ManagedRunnerSettings {
        pidash_db::config::ManagedRunnerSettings {
            enabled: false,
            queued_max_age_secs: 43200,
            graceful_stop_secs: 30,
            sweep_interval_secs: 300,
            desktop_min_version: String::new(),
        }
    }

    struct LiveGraph {
        user: Uuid,
        ws: Uuid,
        project: Uuid,
        pod: Uuid,
        runner: Uuid,
        backlog: Uuid,
        in_progress: Uuid,
        issue: Uuid,
        seed_at: DateTime<Utc>,
    }

    /// Seed a local-runner project + default pod + Backlog / In Progress
    /// states + one issue on Backlog (the fx6 `seed_graph` shape; ids
    /// and slugs are unique per run).
    async fn seed_live_graph(pool: &PgPool) -> LiveGraph {
        let seed_at = DateTime::parse_from_rfc3339("2026-06-01T12:00:00Z")
            .expect("frozen clock")
            .with_timezone(&Utc);
        let uniq = Uuid::new_v4().to_string()[..8].to_owned();
        let user = Uuid::new_v4();
        let ws = Uuid::new_v4();
        let project = Uuid::new_v4();
        let pod = Uuid::new_v4();
        let runner = Uuid::new_v4();
        let backlog = Uuid::new_v4();
        let in_progress = Uuid::new_v4();
        let issue = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, username, email, password, first_name, last_name, \
             display_name, avatar, created_location, last_location, last_login_ip, \
             last_login_medium, last_login_uagent, last_logout_ip, user_timezone, token, \
             is_active, is_bot, is_email_valid, is_email_verified, is_managed, \
             is_password_autoset, is_password_expired, is_password_reset_required, \
             is_staff, is_superuser, created_at, updated_at, date_joined) \
             VALUES ($1, $2, $3, '!', '', '', '', '', '', '', '', 'email', '', '', 'UTC', \
             $4, true, false, false, false, false, false, false, false, false, false, \
             $5, $5, $5)",
        )
        .bind(user)
        .bind(format!("t1124-{uniq}"))
        .bind(format!("t1124-{uniq}@example.com"))
        .bind(Uuid::new_v4().to_string())
        .bind(seed_at)
        .execute(pool)
        .await
        .expect("seed user");
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, background_color, \
             owner_id, slug, timezone) \
             VALUES ($1, $2, $2, $3, '#f6c8dB', $4, $5, 'UTC')",
        )
        .bind(ws)
        .bind(seed_at)
        .bind("T1124 ws")
        .bind(user)
        .bind(format!("t1124-ws-{uniq}"))
        .execute(pool)
        .await
        .expect("seed workspace");
        sqlx::query(
            "INSERT INTO projects (id, created_at, updated_at, name, identifier, description, \
             network, module_view, cycle_view, issue_views_view, page_view, intake_view, \
             is_time_tracking_enabled, is_issue_type_enabled, is_default, \
             guest_view_all_features, members_can_edit_states, archive_in, close_in, \
             logo_props, timezone, repo_url, base_branch, agent_default_interval_seconds, \
             agent_default_max_ticks, agent_review_default_interval_seconds, \
             agent_test_default_interval_seconds, agent_ticking_enabled, \
             default_agent_executor, workspace_id, dedicated_runner_lost_action) \
             VALUES ($1, $2, $2, 'T1124', $3, '', 2, false, false, false, true, false, false, \
             false, false, false, true, 0, 0, '{}', 'UTC', 'https://example.com/t1124.git', \
             'main', 10800, 10, 10800, 10800, true, 'local_runner', $4, 'fail_to_backlog')",
        )
        .bind(project)
        .bind(seed_at)
        .bind(format!("T24{}", &uniq[..6]))
        .bind(ws)
        .execute(pool)
        .await
        .expect("seed project");
        sqlx::query(
            "INSERT INTO pod (id, created_at, updated_at, description, is_default, name, \
             project_id, workspace_id) \
             VALUES ($1, $2, $2, '', true, $3, $4, $5)",
        )
        .bind(pod)
        .bind(seed_at)
        .bind(format!("t1124-pod-{uniq}"))
        .bind(project)
        .bind(ws)
        .execute(pool)
        .await
        .expect("seed pod");
        sqlx::query(
            "INSERT INTO runner (id, owner_id, workspace_id, pod_id, name, created_at, \
             updated_at, capabilities, status, dev_metadata, os, arch, \
             runner_version, protocol_version, host_label, refresh_token_hash, \
             refresh_token_fingerprint, refresh_token_generation, \
             previous_refresh_token_hash, access_token_signing_key_version, \
             enrollment_token_hash, enrollment_token_fingerprint, revoked_reason, \
             visibility, provisioning) \
             VALUES ($1, $2, $3, $4, $5, $6, $6, '[]', 'online', '{}', '', '', '', 1, '', \
             '', '', 0, '', 1, '', '', '', 0, 'manual')",
        )
        .bind(runner)
        .bind(user)
        .bind(ws)
        .bind(pod)
        .bind(format!("t1124-box-{uniq}"))
        .bind(seed_at)
        .execute(pool)
        .await
        .expect("seed runner");
        for (id, name, group, default, sequence) in [
            (backlog, "Backlog", "backlog", true, 0.0),
            (in_progress, "In Progress", "started", false, 1.0),
        ] {
            sqlx::query(
                "INSERT INTO states (id, created_at, updated_at, name, description, color, slug, \
                 project_id, workspace_id, sequence, \"group\", \"default\", is_triage) \
                 VALUES ($1, $2, $2, $3, '', '', $4, $5, $6, $7, $8, $9, false)",
            )
            .bind(id)
            .bind(seed_at)
            .bind(name)
            .bind(format!(
                "t1124-{uniq}-{}",
                name.to_lowercase().replace(' ', "-")
            ))
            .bind(project)
            .bind(ws)
            .bind(sequence)
            .bind(group)
            .bind(default)
            .execute(pool)
            .await
            .expect("seed state");
        }
        sqlx::query(
            "INSERT INTO issues (id, created_at, updated_at, name, description_json, priority, \
             sequence_id, created_by_id, project_id, state_id, workspace_id, description_html, \
             description_stripped, sort_order, is_draft, git_work_branch, workpad, \
             complexity_score) \
             VALUES ($1, $2, $2, 'T1124 orig', '{}', 'none', 1, $3, $4, $5, $6, '', NULL, 0.0, \
             false, '', '', 0)",
        )
        .bind(issue)
        .bind(seed_at)
        .bind(user)
        .bind(project)
        .bind(backlog)
        .bind(ws)
        .execute(pool)
        .await
        .expect("seed issue");
        LiveGraph {
            user,
            ws,
            project,
            pod,
            runner,
            backlog,
            in_progress,
            issue,
            seed_at,
        }
    }

    async fn cleanup_live_graph(pool: &PgPool, graph: &LiveGraph) {
        for (table, column, id) in [
            ("issue_comments", "issue_id", graph.issue),
            ("issue_agent_ticker", "issue_id", graph.issue),
            ("agent_run", "work_item_id", graph.issue),
            ("issues", "id", graph.issue),
            ("states", "id", graph.backlog),
            ("states", "id", graph.in_progress),
            ("runner", "id", graph.runner),
            ("pod", "id", graph.pod),
            ("projects", "id", graph.project),
            ("workspaces", "id", graph.ws),
            ("users", "id", graph.user),
        ] {
            sqlx::query(&format!("DELETE FROM {table} WHERE {column} = $1"))
                .bind(id)
                .execute(pool)
                .await
                .expect("cleanup row");
        }
    }

    /// The fused update is one atomic unit (`issues.py:334-381`): with
    /// the ticker table hidden, the drive fails mid-flight (after the
    /// save) and the issue row must read back unedited — name, state,
    /// and stamps. Pre-fix this fails: the save commits before the
    /// drive runs, so the edit survives. No runners are seeded on
    /// purpose: the bounce path touches the ticker too, so the
    /// injection fires whichever way preflight decides.
    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_fused_update_rolls_back_save_on_middrive_error() {
        let pool = live_pool().await;
        let graph = seed_live_graph(&pool).await;
        let store = PgToolStore::new(
            pool.clone(),
            None,
            live_cloud_settings(),
            live_managed_settings(),
        )
        .expect("build store");
        let now = DateTime::parse_from_rfc3339("2026-07-01T12:00:00Z")
            .expect("frozen clock")
            .with_timezone(&Utc);
        sqlx::query("ALTER TABLE issue_agent_ticker RENAME TO issue_agent_ticker_hidden_1124")
            .execute(&pool)
            .await
            .expect("hide ticker table");
        let patch = IssuePatch {
            name: Some("T1124 edited".to_owned()),
            state_given: true,
            state_id: Some(graph.in_progress),
            ..Default::default()
        };
        let to = pidash_services::orchestration::creation::StateView {
            id: graph.in_progress,
            name: "In Progress".to_owned(),
            group: "started".to_owned(),
        };
        let error = store
            .update_issue_row(&patch, &graph.issue, Some(to), &graph.user, &now)
            .await
            .expect_err("hidden ticker table must fail the drive");
        sqlx::query("ALTER TABLE issue_agent_ticker_hidden_1124 RENAME TO issue_agent_ticker")
            .execute(&pool)
            .await
            .expect("restore ticker table");
        assert!(
            error.message().contains("issue_agent_ticker"),
            "unexpected failure: {}",
            error.message()
        );
        let row: (String, Option<Uuid>, DateTime<Utc>) =
            sqlx::query_as("SELECT name, state_id, updated_at FROM issues WHERE id = $1")
                .bind(graph.issue)
                .fetch_one(&pool)
                .await
                .expect("reread issue");
        cleanup_live_graph(&pool, &graph).await;
        assert_eq!(row.0, "T1124 orig", "name edit must roll back");
        assert_eq!(row.1, Some(graph.backlog), "state edit must roll back");
        assert_eq!(row.2, graph.seed_at, "audit stamps must roll back");
    }

    /// `create_comment_row` binds `attachments` as a typed empty
    /// `varchar[]` (`issue.py:563`), not jsonb: the comment INSERT, the
    /// `Description` INSERT and the `description_id` link all land, and
    /// the stored `attachments` reads back `'{}'` like Django's
    /// `create_comment` default. Pre-fix this fails: sqlx sends the
    /// `Value::Array` bind as jsonb and Postgres cannot cast jsonb to
    /// `varchar[]` (PIDASHCONV-1582).
    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_create_comment_row_stores_empty_varchar_attachments() {
        let pool = live_pool().await;
        let graph = seed_live_graph(&pool).await;
        let store = PgToolStore::new(
            pool.clone(),
            None,
            live_cloud_settings(),
            live_managed_settings(),
        )
        .expect("build store");
        let now = DateTime::parse_from_rfc3339("2026-07-01T12:00:00Z")
            .expect("frozen clock")
            .with_timezone(&Utc);
        let input = NewCommentInput {
            issue_id: graph.issue,
            project_id: graph.project,
            workspace_id: graph.ws,
            actor_id: graph.user,
            html: "<p>hello</p>".to_owned(),
            fold: false,
        };
        let comment_id = store
            .create_comment_row(&input, &now)
            .await
            .expect("comment insert must succeed");
        let row: (Vec<String>, Option<Uuid>, String) = sqlx::query_as(
            "SELECT attachments, description_id, comment_stripped \
             FROM issue_comments WHERE id = $1",
        )
        .bind(comment_id)
        .fetch_one(&pool)
        .await
        .expect("reread comment");
        let description_id = row.1.expect("description link must be set");
        let desc: (String, Option<String>) = sqlx::query_as(
            "SELECT description_html, description_stripped FROM descriptions WHERE id = $1",
        )
        .bind(description_id)
        .fetch_one(&pool)
        .await
        .expect("reread description");
        // Comment before description (the link), description before the
        // graph (its project FK) — then the shared graph cleanup.
        sqlx::query("DELETE FROM issue_comments WHERE id = $1")
            .bind(comment_id)
            .execute(&pool)
            .await
            .expect("cleanup comment");
        sqlx::query("DELETE FROM descriptions WHERE id = $1")
            .bind(description_id)
            .execute(&pool)
            .await
            .expect("cleanup description");
        cleanup_live_graph(&pool, &graph).await;
        assert!(
            row.0.is_empty(),
            "attachments must be '{{}}', got {:?}",
            row.0
        );
        assert_eq!(row.2, "hello", "MLStripper comment_stripped");
        assert_eq!(desc.0, "<p>hello</p>", "description keeps the HTML");
        assert_eq!(desc.1.as_deref(), Some("hello"), "strip_tags row");
    }

    /// The dedicated-runner fused path (PDASHOSS01-350): a runner patch
    /// stamps runner + pod (pod follows the runner) on the same
    /// transaction, `list_project_runners` returns the visible row with
    /// its assignability fields, resolution sees the runner, and a
    /// pod-only patch clears the dedicated runner.
    #[tokio::test]
    #[ignore = "needs migrated scratch DB via DATABASE_URL"]
    async fn live_fused_runner_apply_and_list() {
        let pool = live_pool().await;
        let graph = seed_live_graph(&pool).await;
        let store = PgToolStore::new(
            pool.clone(),
            None,
            live_cloud_settings(),
            live_managed_settings(),
        )
        .expect("build store");
        let now = DateTime::parse_from_rfc3339("2026-07-01T12:00:00Z")
            .expect("frozen clock")
            .with_timezone(&Utc);

        // Resolution + runner read before any assignment.
        let resolved = store
            .list_runners_for_resolution(&graph.project, &graph.user)
            .await
            .expect("resolution list");
        assert!(
            resolved
                .iter()
                .any(|(id, name)| *id == graph.runner && name.starts_with("t1124-box-")),
            "seeded runner must resolve by name: {resolved:?}"
        );
        let listed = store
            .list_project_runners(&graph.project, &graph.user, Some(&graph.issue.to_string()))
            .await
            .expect("list project runners");
        let row = listed
            .iter()
            .find(|row| row["id"] == serde_json::json!(graph.runner.to_string()))
            .expect("seeded runner listed");
        assert_eq!(row["assignable"], true);
        assert_eq!(row["is_busy"], false);
        assert_eq!(row["queue_depth"], 0);

        // The shared E-code validation accepts the seeded runner.
        store
            .validate_execution_target_write(
                &pidash_services::domain::issue_assignment::ExecutionTargetWrite {
                    project_id: Some(graph.project),
                    issue_id: Some(graph.issue),
                    pod_id: None,
                    runner_present: true,
                    runner_id: Some(graph.runner),
                    user_id: Some(graph.user),
                },
            )
            .await
            .expect("validation must pass for a visible online runner");

        // Fused runner patch: stamps runner + pod (pod follows runner).
        let patch = IssuePatch {
            apply_runner: true,
            new_runner_id: Some(graph.runner),
            ..Default::default()
        };
        let (stamped, _) = store
            .update_issue_row(&patch, &graph.issue, None, &graph.user, &now)
            .await
            .expect("fused runner patch must succeed");
        assert_eq!(stamped.assigned_runner_id, Some(graph.runner));
        assert_eq!(stamped.assigned_pod_id, Some(graph.pod));

        // The execution detail carries the runner name.
        let detail = store
            .execution_detail(&graph.issue, &graph.user)
            .await
            .expect("execution detail");
        assert_eq!(
            detail.runner_detail["name"],
            serde_json::json!(resolved[0].1)
        );

        // Pod-only patch clears the dedicated runner.
        let patch = IssuePatch {
            pod_touched: true,
            pod_id: Some(graph.pod),
            ..Default::default()
        };
        let (cleared, _) = store
            .update_issue_row(&patch, &graph.issue, None, &graph.user, &now)
            .await
            .expect("pod-only patch must succeed");
        assert_eq!(cleared.assigned_runner_id, None);
        assert_eq!(cleared.assigned_pod_id, Some(graph.pod));

        // The fused apply writes `issue_activities` rows, which the
        // shared graph cleanup cannot see.
        sqlx::query("DELETE FROM issue_activities WHERE issue_id = $1")
            .bind(graph.issue)
            .execute(&pool)
            .await
            .expect("cleanup activities");
        cleanup_live_graph(&pool, &graph).await;
    }
}
