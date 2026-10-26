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
//! [`PgToolStore`] is the sqlx implementation, reusing the services SQL
//! fragments (`MEMBER_PROJECTS_SQL`, `SCOPED_ISSUES_SQL`,
//! `PROJECT_WRITE_GATE_SQL`, the FTS/filter builders, the relation
//! statements) and the jobs orchestration drivers. Unit tests drive the
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct PairRow {
    pub id: Uuid,
    pub issue_id: Uuid,
    pub related_issue_id: Uuid,
    pub relation_type: String,
}

/// `query_as` tuple shapes (one per multi-column read without a
/// dedicated row struct).
type CommentRowTuple = (
    Option<String>,
    DateTime<Utc>,
    String,
    Option<Uuid>,
    Option<String>,
    Option<String>,
);
type MemberRowTuple = (Option<Uuid>, Option<String>, Option<String>, i16);
type GroupedReadTuple = (Uuid, String, i32, String, Option<String>, Option<String>);
type LockedIssueTuple = (Uuid, Option<Uuid>, Option<String>, Option<String>);

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

/// Inputs to the issue create (`issues.create_issue :265-277` plus the
/// `Issue.save` housekeeping the store applies).
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
    /// path.
    async fn drive_transition(
        &self,
        issue_id: &Uuid,
        from_state: Option<pidash_services::orchestration::creation::StateView>,
        to_state: pidash_services::orchestration::creation::StateView,
        actor_id: &Uuid,
        created_by: Option<Uuid>,
        now: &DateTime<Utc>,
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
    /// failure swallowed); only a *lookup* storage failure propagates,
    /// exactly the `try` placement in `fire_state_transition` (the
    /// `fire_after_save` precedent).
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

use pidash_services::assistant::tools_issues::{
    ListFilterPredicates, ISSUE_FTS_SQL, PROJECT_WRITE_GATE_SQL,
};

/// Issue columns the brief/detail shapes read, over the `issues` alias of
/// [`SCOPED_ISSUES_SQL`]-family queries plus the joined project
/// identifier and state name/group (`select_related("project", "state")`).
const ISSUE_ROW_COLUMNS: &str =
    "issues.id, issues.project_id, projects.identifier AS project_identifier, \
     issues.workspace_id, issues.sequence_id, issues.name, issues.parent_id, issues.state_id, \
     states.name AS state_name, states.group AS state_group, issues.priority, \
     issues.description_stripped, issues.created_by_id, issues.updated_at";

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

/// FROM/WHERE half of `tools_issues::SCOPED_ISSUES_SQL` (which selects
/// `issues.*`): the row readers need the joined project/state columns
/// too, so they pair this fragment with [`ISSUE_ROW_COLUMNS`]. The
/// `sql_fragments_match_services_consts` test pins the two texts equal
/// (whitespace-insensitive), so the services const stays the one
/// reviewed source.
const SCOPED_ISSUES_FILTER: &str = "FROM issues \
     INNER JOIN projects ON projects.id = issues.project_id \
     INNER JOIN workspaces ON workspaces.id = issues.workspace_id \
     INNER JOIN project_members ON (project_members.project_id = projects.id \
     AND project_members.member_id = $1 AND project_members.is_active) \
     LEFT OUTER JOIN states ON states.id = issues.state_id \
     WHERE workspaces.slug = $2 \
     AND issues.deleted_at IS NULL \
     AND NOT (states.group = 'triage' AND states.group IS NOT NULL) \
     AND NOT (issues.archived_at IS NOT NULL) \
     AND NOT (projects.archived_at IS NOT NULL) \
     AND NOT (issues.is_draft)";

/// FROM/WHERE half of `tools_issues::MEMBER_PROJECTS_SQL`, same pinning deal.
const MEMBER_PROJECTS_FILTER: &str = "FROM projects \
     INNER JOIN project_members ON (project_members.project_id = projects.id \
     AND project_members.member_id = $1 AND project_members.is_active) \
     INNER JOIN workspaces ON workspaces.id = projects.workspace_id \
     WHERE workspaces.slug = $2 AND projects.deleted_at IS NULL";

/// Decode one [`ISSUE_ROW_COLUMNS`] row from an untyped query result.
fn decode_issue_row(row: &sqlx::postgres::PgRow) -> Result<IssueRow, StoreError> {
    use sqlx::Row;
    Ok(IssueRow {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        project_identifier: row.try_get("project_identifier")?,
        workspace_id: row.try_get("workspace_id")?,
        sequence_id: i64::from(row.try_get::<i32, _>("sequence_id")?),
        name: row.try_get("name")?,
        parent_id: row.try_get("parent_id")?,
        state_id: row.try_get("state_id")?,
        state_name: row.try_get("state_name")?,
        state_group: row.try_get("state_group")?,
        priority: row.try_get("priority")?,
        description_stripped: row.try_get("description_stripped")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_at: row.try_get("updated_at")?,
    })
}

type IssueRowTuple = (
    Uuid,
    Uuid,
    String,
    Uuid,
    i32,
    String,
    Option<Uuid>,
    Option<Uuid>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<Uuid>,
    DateTime<Utc>,
);

fn issue_row_from_tuple(row: IssueRowTuple) -> IssueRow {
    IssueRow {
        id: row.0,
        project_id: row.1,
        project_identifier: row.2,
        workspace_id: row.3,
        sequence_id: i64::from(row.4),
        name: row.5,
        parent_id: row.6,
        state_id: row.7,
        state_name: row.8,
        state_group: row.9,
        priority: row.10,
        description_stripped: row.11,
        created_by_id: row.12,
        updated_at: row.13,
    }
}

impl ToolStore for PgToolStore {
    async fn require_user(&self, deps: &AssistantDeps) -> Result<Uuid, StoreError> {
        let id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE id = $1")
            .bind(deps.user_id)
            .fetch_optional(&self.pool)
            .await?;
        id.ok_or(StoreError::Missing { model: "User" })
    }

    async fn member_projects(&self, deps: &AssistantDeps) -> Result<Vec<ProjectRow>, StoreError> {
        // `DISTINCT` (`_scoping.py:45-49`): remove+re-add leaves two
        // `is_active` rows (soft-delete never clears it); without it
        // the project lists twice.
        let rows: Vec<(Uuid, String, String, Uuid, bool)> = sqlx::query_as(&format!(
            "SELECT DISTINCT projects.id, projects.identifier, projects.name, \
             projects.workspace_id, projects.guest_view_all_features \
             {MEMBER_PROJECTS_FILTER} ORDER BY projects.name LIMIT 50"
        ))
        .bind(deps.user_id)
        .bind(&deps.workspace_slug)
        .fetch_all(&self.pool)
        .await?;
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
        let row: Option<(Uuid, String, String, Uuid, bool)> = sqlx::query_as(&format!(
            "SELECT projects.id, projects.identifier, projects.name, projects.workspace_id, \
             projects.guest_view_all_features {MEMBER_PROJECTS_FILTER} AND projects.id = $3"
        ))
        .bind(deps.user_id)
        .bind(&deps.workspace_slug)
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?;
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
        let row: Option<IssueRowTuple> = sqlx::query_as(&format!(
            "SELECT {ISSUE_ROW_COLUMNS} {SCOPED_ISSUES_FILTER} AND issues.id = $3"
        ))
        .bind(deps.user_id)
        .bind(&deps.workspace_slug)
        .bind(issue_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(issue_row_from_tuple))
    }

    async fn search_issues_page(
        &self,
        deps: &AssistantDeps,
        project_id: Option<Uuid>,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<IssueRow>, StoreError> {
        use pidash_services::assistant::tools_issues::{
            escape_icontains, search_sequence_tokens, search_uses_fts,
        };
        // `issues.search_issues :65-74`: scope, optional project
        // narrowing, FTS only for non-blank queries, `.distinct()`,
        // `-updated_at`, the lookahead window.
        let mut sql = format!("SELECT DISTINCT {ISSUE_ROW_COLUMNS} {SCOPED_ISSUES_FILTER}");
        if project_id.is_some() {
            sql.push_str(" AND issues.project_id = $3");
        }
        let mut next_param: i32 = if project_id.is_some() { 4 } else { 3 };
        let use_fts = search_uses_fts(query);
        if use_fts {
            // [`ISSUE_FTS_SQL`] numbers its own binds $1/$2/$3; shift
            // them past the scope/project binds.
            let fts = ISSUE_FTS_SQL
                .replace("$1", &format!("${}", next_param))
                .replace("$2", &format!("${}", next_param + 1))
                .replace("$3", &format!("${}", next_param + 2));
            sql.push_str(&format!(" AND {fts}"));
            next_param += 3;
        }
        sql.push_str(&format!(
            " ORDER BY issues.updated_at DESC LIMIT ${} OFFSET ${}",
            next_param,
            next_param + 1
        ));
        let mut statement = sqlx::query(&sql)
            .bind(deps.user_id)
            .bind(&deps.workspace_slug);
        if let Some(project) = project_id {
            statement = statement.bind(project);
        }
        if use_fts {
            let tokens = search_sequence_tokens(query);
            statement = statement
                .bind(query)
                .bind(tokens)
                .bind(escape_icontains(query));
        }
        let rows = statement
            .bind(limit + 1)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(decode_issue_row).collect()
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
        let mut params: Vec<String> = Vec::new();
        let rendered = filters.to_sql(&mut params, 4);
        let mut sql = format!(
            "SELECT DISTINCT {ISSUE_ROW_COLUMNS} {SCOPED_ISSUES_FILTER} AND issues.project_id = $3"
        );
        if !rendered.is_empty() {
            sql.push_str(&format!(" AND {rendered}"));
        }
        let next_param = 4 + params.len() as i32;
        sql.push_str(&format!(
            " ORDER BY issues.updated_at DESC LIMIT ${next_param} OFFSET ${}",
            next_param + 1
        ));
        let mut statement = sqlx::query(&sql)
            .bind(deps.user_id)
            .bind(&deps.workspace_slug)
            .bind(project_id);
        for param in &params {
            statement = statement.bind(param);
        }
        let rows = statement
            .bind(limit + 1)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(decode_issue_row).collect()
    }

    async fn my_issues_page(
        &self,
        deps: &AssistantDeps,
        scope: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<IssueRow>, StoreError> {
        use pidash_services::assistant::tools_issues::my_issues_scope_sql;
        // `issues.list_my_issues :143`: involvement filter inside the
        // member scope, `-updated_at`, lookahead window. The scope
        // fragment binds `$1` (user id) again — same value, new slot.
        let fragment = my_issues_scope_sql(
            pidash_services::assistant::tools_scoping::IssueScope::parse(scope).as_str(),
        )
        .replace("$1", "$3");
        let sql = format!(
            "SELECT DISTINCT {ISSUE_ROW_COLUMNS} {SCOPED_ISSUES_FILTER} AND {fragment} \
             ORDER BY issues.updated_at DESC LIMIT $4 OFFSET $5"
        );
        let rows = sqlx::query(&sql)
            .bind(deps.user_id)
            .bind(&deps.workspace_slug)
            .bind(deps.user_id)
            .bind(limit + 1)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(decode_issue_row).collect()
    }

    async fn issue_comments(&self, issue_id: &Uuid) -> Result<Vec<IssueCommentRow>, StoreError> {
        let rows: Vec<CommentRowTuple> =
            sqlx::query_as(
                "SELECT issue_comments.comment_stripped, issue_comments.created_at, \
                 issue_comments.speaker_type, issue_comments.actor_id, users.display_name, users.email \
                 FROM issue_comments LEFT OUTER JOIN users ON users.id = issue_comments.actor_id \
                 WHERE issue_comments.issue_id = $1 AND issue_comments.deleted_at IS NULL \
                 ORDER BY issue_comments.created_at DESC LIMIT 10",
            )
            .bind(issue_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    comment_stripped,
                    created_at,
                    speaker_type,
                    actor_id,
                    actor_display_name,
                    actor_email,
                )| {
                    IssueCommentRow {
                        comment_stripped,
                        created_at,
                        speaker_type,
                        actor_id,
                        actor_display_name,
                        actor_email,
                    }
                },
            )
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
        let rows: Vec<(Uuid, String, String, bool, f64)> = sqlx::query_as(
            "SELECT states.id, states.name, states.group, states.default, states.sequence \
             FROM states INNER JOIN workspaces ON workspaces.id = states.workspace_id \
             WHERE states.project_id = $1 AND workspaces.slug = $2 \
             AND states.deleted_at IS NULL AND NOT (states.group = 'triage') \
             ORDER BY states.sequence",
        )
        .bind(project_id)
        .bind(&deps.workspace_slug)
        .fetch_all(&self.pool)
        .await?;
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
        let rows: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT labels.id, labels.name FROM labels \
             INNER JOIN workspaces ON workspaces.id = labels.workspace_id \
             WHERE labels.project_id = $1 AND workspaces.slug = $2 AND labels.deleted_at IS NULL \
             ORDER BY labels.name",
        )
        .bind(project_id)
        .bind(&deps.workspace_slug)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name)| LabelRow { id, name })
            .collect())
    }

    async fn workspace_labels(&self, deps: &AssistantDeps) -> Result<Vec<LabelRow>, StoreError> {
        let rows: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT labels.id, labels.name FROM labels \
             INNER JOIN workspaces ON workspaces.id = labels.workspace_id \
             WHERE labels.project_id IS NULL AND workspaces.slug = $1 \
             AND labels.deleted_at IS NULL",
        )
        .bind(&deps.workspace_slug)
        .fetch_all(&self.pool)
        .await?;
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
        // `::int` cast makes out-of-int4 tails raise exactly as
        // Django's int4 bind does (miss-vs-500 parity).
        let row: Option<IssueRowTuple> = sqlx::query_as(&format!(
            "SELECT {ISSUE_ROW_COLUMNS} FROM issues \
             INNER JOIN projects ON projects.id = issues.project_id \
             INNER JOIN workspaces ON workspaces.id = issues.workspace_id \
             LEFT OUTER JOIN states ON states.id = issues.state_id \
             WHERE workspaces.slug = $1 AND UPPER(projects.identifier) = UPPER($2) \
             AND issues.sequence_id = $3::int AND issues.deleted_at IS NULL \
             AND NOT (states.group = 'triage' AND states.group IS NOT NULL) AND NOT (issues.archived_at IS NOT NULL) \
             AND NOT (projects.archived_at IS NOT NULL) AND NOT (issues.is_draft) \
             ORDER BY issues.created_at DESC LIMIT 1"
        ))
        .bind(&deps.workspace_slug)
        .bind(project_ident)
        .bind(sequence_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(issue_row_from_tuple))
    }

    async fn project_members(
        &self,
        deps: &AssistantDeps,
        project_id: &Uuid,
    ) -> Result<Vec<MemberRow>, StoreError> {
        // `role` is a `PositiveSmallIntegerField` (int2): decode i16,
        // widen for the row.
        let rows: Vec<MemberRowTuple> = sqlx::query_as(
            "SELECT project_members.member_id, users.display_name, users.email, project_members.role \
             FROM project_members \
             INNER JOIN workspaces ON workspaces.id = project_members.workspace_id \
             LEFT OUTER JOIN users ON users.id = project_members.member_id \
             WHERE project_members.project_id = $1 AND workspaces.slug = $2 \
             AND project_members.is_active AND project_members.deleted_at IS NULL \
             ORDER BY project_members.created_at DESC",
        )
        .bind(project_id)
        .bind(&deps.workspace_slug)
        .fetch_all(&self.pool)
        .await?;
        // `if m.member is None: continue` (`projects.py:56-57`).
        Ok(rows
            .into_iter()
            .filter_map(|(member_id, display_name, email, role)| {
                member_id.map(|member_id| MemberRow {
                    member_id,
                    display_name,
                    email,
                    role: i32::from(role),
                })
            })
            .collect())
    }

    async fn check_project_write(
        &self,
        deps: &AssistantDeps,
        project_id: &Uuid,
    ) -> Result<bool, StoreError> {
        let allowed: Option<bool> = sqlx::query_scalar(PROJECT_WRITE_GATE_SQL)
            .bind(deps.user_id)
            .bind(&deps.workspace_slug)
            .bind(project_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(allowed.unwrap_or(false))
    }

    async fn agent_runs(&self, issue_id: &Uuid) -> Result<Vec<AgentRunRow>, StoreError> {
        let rows: Vec<(Uuid, String, Option<DateTime<Utc>>)> = sqlx::query_as(
            "SELECT id, status, created_at FROM agent_run \
             WHERE work_item_id = $1 ORDER BY created_at DESC LIMIT 5",
        )
        .bind(issue_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, status, created_at)| AgentRunRow {
                id,
                status,
                created_at,
            })
            .collect())
    }

    async fn scoped_issue_by_identifier(
        &self,
        deps: &AssistantDeps,
        project_ident: &str,
        sequence_id: i64,
    ) -> Result<Option<IssueRow>, StoreError> {
        // `.first()` under `Issue.Meta.ordering` (`-created_at`);
        // the `::int` cast keeps int4-range parity with Django's bind
        // (out-of-range tails raise instead of missing).
        let sql = format!(
            "SELECT {ISSUE_ROW_COLUMNS} {SCOPED_ISSUES_FILTER} \
             AND UPPER(projects.identifier) = UPPER($3) AND issues.sequence_id = $4::int \
             ORDER BY issues.created_at DESC LIMIT 1"
        );
        let rows = sqlx::query(&sql)
            .bind(deps.user_id)
            .bind(&deps.workspace_slug)
            .bind(project_ident)
            .bind(sequence_id)
            .fetch_all(&self.pool)
            .await?;
        rows.first().map(decode_issue_row).transpose()
    }

    async fn pair_rows(&self, a_id: &Uuid, b_id: &Uuid) -> Result<Vec<PairRow>, StoreError> {
        let rows: Vec<(Uuid, Uuid, Uuid, String)> =
            sqlx::query_as(pidash_services::assistant::tools_issues::PAIR_ROWS_SQL)
                .bind(a_id)
                .bind(b_id)
                .fetch_all(&self.pool)
                .await?;
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
        let outcome = sqlx::query(
            "INSERT INTO issue_relations (id, created_at, updated_at, created_by_id, updated_by_id, \
             deleted_at, project_id, workspace_id, issue_id, related_issue_id, relation_type) \
             VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, $9)",
        )
        .bind(Uuid::new_v4())
        .bind(now)
        .bind(now)
        .bind(actor_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(issue_id)
        .bind(related_id)
        .bind(stored_type)
        .execute(&self.pool)
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
        sqlx::query(
            "UPDATE issue_relations SET deleted_at = $1, updated_at = $2, updated_by_id = $3 \
             WHERE id = $4",
        )
        .bind(now)
        .bind(now)
        .bind(actor_id)
        .bind(row_id)
        .execute(&self.pool)
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
        let rows: Vec<(Uuid, Uuid, Uuid, String)> =
            sqlx::query_as(pidash_services::assistant::tools_issues::GROUPED_RELATIONS_SQL)
                .bind(issue_id)
                .bind(workspace_id)
                .fetch_all(&self.pool)
                .await?;
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
        let rows: Vec<GroupedReadTuple> = sqlx::query_as(&format!(
            "SELECT issues.id, projects.identifier, issues.sequence_id, issues.name, \
                 states.name, states.group {SCOPED_ISSUES_FILTER} \
                 AND issues.id = ANY($3) AND issues.workspace_id = $4"
        ))
        .bind(deps.user_id)
        .bind(&deps.workspace_slug)
        .bind(ids)
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, project_identifier, sequence_id, name, state_name, state_group)| {
                    GroupedIssueRow {
                        id,
                        project_identifier,
                        sequence_id: i64::from(sequence_id),
                        name,
                        state_name,
                        state_group,
                    }
                },
            )
            .collect())
    }

    async fn issue_parent_id(&self, issue_id: &Uuid) -> Result<Option<Uuid>, StoreError> {
        // `Issue.objects.filter(pk).values_list("parent_id").first()`:
        // the plain manager (soft-delete only, triage included).
        let parent: Option<Option<Uuid>> =
            sqlx::query_scalar("SELECT parent_id FROM issues WHERE id = $1 AND deleted_at IS NULL")
                .bind(issue_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(parent.flatten())
    }

    async fn create_issue_row(
        &self,
        input: &NewIssueInput,
        now: &DateTime<Utc>,
    ) -> Result<IssueRow, StoreError> {
        use pidash_db::ops::repair::{convert_uuid_to_integer, ADVISORY_LOCK_SQL};
        let mut tx = self.pool.begin().await?;
        // The tool's `Project.objects.select_for_update().get(...)`
        // (`issues.py:259`); missing is `DoesNotExist` → internal.
        let locked: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM projects WHERE id = $1 AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(input.project_id)
        .fetch_optional(&mut *tx)
        .await?;
        if locked.is_none() {
            return Err(StoreError::Missing { model: "Project" });
        }
        // The tool's `MAX(sequence_id)+1` read (`issues.py:260-264`) is
        // intentionally not repeated: `Issue.save` overwrites the
        // passed `sequence_id` from `issue_sequences` (`issue.py:313`),
        // so the read is unobservable (a lock-free SELECT).
        sqlx::query(ADVISORY_LOCK_SQL)
            .bind(convert_uuid_to_integer(&input.project_id))
            .execute(&mut *tx)
            .await?;
        // `IssueSequence.objects` is the soft-delete manager, and
        // `sequence` is a `PositiveBigIntegerField` (int8).
        let last: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(sequence) FROM issue_sequences \
             WHERE project_id = $1 AND deleted_at IS NULL",
        )
        .bind(input.project_id)
        .fetch_one(&mut *tx)
        .await?;
        let sequence: i64 = last.map(|value| value.saturating_add(1)).unwrap_or(1);
        // `Issue.save` None-state resolution (`issue.py:286-301`):
        // default non-triage state, else the first non-triage row
        // (`.first()` under `State.Meta.ordering`, so lowest
        // `sequence`), else None. Both the `is_triage` filter from the
        // query expression and the `StateManager` group exclusion apply:
        // seeded Triage rows carry `is_triage = FALSE` (the project seed
        // never sets it), so the column alone would admit them.
        let state_id =
            match input.state_id {
                Some(id) => Some(id),
                None => {
                    let default: Option<Uuid> = sqlx::query_scalar(
                        "SELECT id FROM states WHERE project_id = $1 AND is_triage = FALSE \
                         AND deleted_at IS NULL AND NOT (states.group = 'triage') \
                         AND states.default ORDER BY sequence LIMIT 1",
                    )
                    .bind(input.project_id)
                    .fetch_optional(&mut *tx)
                    .await?;
                    match default {
                        Some(id) => Some(id),
                        None => sqlx::query_scalar(
                            "SELECT id FROM states WHERE project_id = $1 AND is_triage = FALSE \
                             AND deleted_at IS NULL AND NOT (states.group = 'triage') \
                             ORDER BY sequence LIMIT 1",
                        )
                        .bind(input.project_id)
                        .fetch_optional(&mut *tx)
                        .await?,
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
            let row: Option<(String, String)> =
                sqlx::query_as("SELECT name, states.group FROM states WHERE id = $1")
                    .bind(id)
                    .fetch_optional(&mut *tx)
                    .await?;
            if let Some((name, group)) = row {
                if input.state_id.is_some() && group == "completed" {
                    completed_at = Some(*now);
                }
                state_name = Some(name);
                state_group = Some(group);
            }
        }
        // `assigned_pod` (`issue.py:276-284`): the project's default pod.
        let assigned_pod: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM pod WHERE project_id = $1 AND is_default AND deleted_at IS NULL LIMIT 1",
        )
        .bind(input.project_id)
        .fetch_optional(&mut *tx)
        .await?;
        // `sort_order` (`issue.py:333-340`): max in (project, state) +
        // 10000, else the 65535 default. `Issue.objects` (soft-delete
        // only) — including triage/archived/draft rows.
        let largest: Option<f64> = if let Some(id) = state_id {
            sqlx::query_scalar(
                "SELECT MAX(sort_order) FROM issues \
                 WHERE project_id = $1 AND state_id = $2 AND deleted_at IS NULL",
            )
            .bind(input.project_id)
            .bind(id)
            .fetch_one(&mut *tx)
            .await?
        } else {
            sqlx::query_scalar(
                "SELECT MAX(sort_order) FROM issues \
                 WHERE project_id = $1 AND state_id IS NULL AND deleted_at IS NULL",
            )
            .bind(input.project_id)
            .fetch_one(&mut *tx)
            .await?
        };
        let sort_order = largest.map(|value| value + 10000.0).unwrap_or(65535.0);
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
        sqlx::query(
            "INSERT INTO issues (id, created_at, updated_at, deleted_at, point, name, \
             description_json, description_html, description_stripped, description_binary, \
             priority, complexity_score, start_date, target_date, sequence_id, sort_order, \
             completed_at, archived_at, is_draft, external_source, external_id, git_work_branch, \
             workpad, created_via, agent_executor, created_by_id, updated_by_id, project_id, \
             workspace_id, parent_id, state_id, estimate_point_id, type_id, assigned_pod_id) \
             VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7, NULL, $8, 0, NULL, NULL, $9, $10, \
             $11, NULL, FALSE, NULL, NULL, '', '', $12, NULL, $13, NULL, $14, $15, $16, $17, \
             NULL, NULL, $18)",
        )
        .bind(issue_id)
        .bind(now)
        .bind(now)
        .bind(&input.name)
        .bind(Value::Object(serde_json::Map::new()))
        .bind(&input.description_html)
        .bind(&stripped)
        .bind(&input.priority)
        .bind(sequence)
        .bind(sort_order)
        .bind(completed_at)
        .bind(&input.created_via)
        .bind(input.created_by)
        .bind(input.project_id)
        .bind(input.workspace_id)
        .bind(input.parent_id)
        .bind(state_id)
        .bind(assigned_pod)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO issue_sequences (id, created_at, updated_at, created_by_id, updated_by_id, \
             deleted_at, project_id, workspace_id, issue_id, sequence, deleted) \
             VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, FALSE)",
        )
        .bind(Uuid::new_v4())
        .bind(now)
        .bind(now)
        .bind(input.created_by)
        .bind(input.project_id)
        .bind(input.workspace_id)
        .bind(issue_id)
        .bind(sequence)
        .execute(&mut *tx)
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
        let key_row: Option<Option<Vec<u8>>> = sqlx::query_scalar(
            "SELECT api_key_encrypted FROM assistant_user_llm_config WHERE user_id = $1 LIMIT 1",
        )
        .bind(actor_id)
        .fetch_optional(&self.pool)
        .await?;
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
        // missing is `DoesNotExist` → internal. The states join reads
        // the locked row's pre-save name/group (`from_state =
        // locked.state`, `issues.py:339`) — the pre-lock read may be
        // stale under a concurrent edit.
        let locked: Option<LockedIssueTuple> = sqlx::query_as(
            "SELECT issues.id, issues.state_id, states.name, states.group FROM issues \
             LEFT OUTER JOIN states ON states.id = issues.state_id \
             WHERE issues.id = $1 AND issues.deleted_at IS NULL FOR UPDATE OF issues",
        )
        .bind(issue_id)
        .fetch_optional(&mut **tx.inner())
        .await?;
        let Some((_, from_id, from_name, from_group)) = locked else {
            return Err(StoreError::Missing { model: "Issue" });
        };
        // `locked.save(update_fields=...)` (`issues.py:337-361`):
        // exactly the tool's columns. `description_stripped` is
        // recomputed in memory but NOT written (it is absent from
        // `update_fields`), so a description edit leaves the column
        // stale — ported as-is. `completed_at` is untouched (the
        // create-only `adding` arm never runs).
        let mut sets: Vec<String> = Vec::new();
        if patch.name.is_some() {
            sets.push(format!("name = ${}", sets.len() + 1));
        }
        if patch.description_html.is_some() {
            sets.push(format!("description_html = ${}", sets.len() + 1));
            sets.push(format!("description_json = ${}", sets.len() + 1));
        }
        if patch.priority.is_some() {
            sets.push(format!("priority = ${}", sets.len() + 1));
        }
        if patch.state_given {
            sets.push(format!("state_id = ${}", sets.len() + 1));
        }
        if patch.parent_touched {
            sets.push(format!("parent_id = ${}", sets.len() + 1));
        }
        sets.push(format!("updated_at = ${}", sets.len() + 1));
        sets.push(format!("updated_by_id = ${}", sets.len() + 1));
        let sql = format!(
            "UPDATE issues SET {} WHERE id = ${}",
            sets.join(", "),
            sets.len() + 1
        );
        let mut statement = sqlx::query(&sql);
        if let Some(name) = &patch.name {
            statement = statement.bind(name);
        }
        if let Some(html) = &patch.description_html {
            statement = statement
                .bind(html)
                .bind(Value::Object(serde_json::Map::new()));
        }
        if let Some(priority) = &patch.priority {
            statement = statement.bind(priority);
        }
        if patch.state_given {
            statement = statement.bind(patch.state_id);
        }
        if patch.parent_touched {
            statement = statement.bind(patch.parent_id);
        }
        statement = statement.bind(now).bind(actor_id).bind(issue_id);
        statement.execute(&mut **tx.inner()).await?;
        // Post-save read (unscoped: scope was verified pre-lock; this
        // mirrors the in-memory `locked` row plus joined names).
        let row: Option<IssueRowTuple> = sqlx::query_as(&format!(
            "SELECT {ISSUE_ROW_COLUMNS} FROM issues \
             INNER JOIN projects ON projects.id = issues.project_id \
             LEFT OUTER JOIN states ON states.id = issues.state_id \
             WHERE issues.id = $1"
        ))
        .bind(issue_id)
        .fetch_optional(&mut **tx.inner())
        .await?;
        let mut row = row
            .map(issue_row_from_tuple)
            .ok_or(StoreError::Missing { model: "Issue" })?;
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
                pidash_api::v1_work_items::handlers_actions::LiveMatcher::new(self.pool.clone());
            let triggered_by =
                pidash_db::dispatch::status::AgentRunTrigger::StateTransition.value();
            sqlx::query("SAVEPOINT turn_preflight_probe")
                .execute(&mut **tx.inner())
                .await?;
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
            sqlx::query("ROLLBACK TO SAVEPOINT turn_preflight_probe")
                .execute(&mut **tx.inner())
                .await?;
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
            let fresh: Option<IssueRowTuple> = sqlx::query_as(&format!(
                "SELECT {ISSUE_ROW_COLUMNS} FROM issues \
                 INNER JOIN projects ON projects.id = issues.project_id \
                 LEFT OUTER JOIN states ON states.id = issues.state_id \
                 WHERE issues.id = $1"
            ))
            .bind(issue_id)
            .fetch_optional(&mut **tx.inner())
            .await?;
            row = fresh
                .map(issue_row_from_tuple)
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
            for run_id in dispatches {
                pidash_jobs::dispatch::dispatch_agent_run(&self.pool, &self.cloud, run_id, *now)
                    .await
                    .map_err(StoreError::from)?;
            }
            for consume in &deferred {
                pidash_services::dispatch::admission::consume_admission_token(&cache, consume);
            }
            if !terminal_effects.is_empty() {
                // Python's transition never finalizes a run: fail closed.
                return Err(StoreError::Transition(
                    "turn transition finalized a run: unreachable, no finalization on this path"
                        .to_owned(),
                ));
            }
        }
        Ok((row, drive_outcome))
    }

    async fn drive_transition(
        &self,
        issue_id: &Uuid,
        from_state: Option<pidash_services::orchestration::creation::StateView>,
        to_state: pidash_services::orchestration::creation::StateView,
        actor_id: &Uuid,
        created_by: Option<Uuid>,
        now: &DateTime<Utc>,
    ) -> Result<TransitionOutcome, StoreError> {
        use pidash_jobs::admission::RedisAdmissionCache;
        use pidash_jobs::orchestration::{LiveCreationStore, StoreDeps};
        use pidash_services::orchestration::entries::{
            handle_issue_state_transition, TransitionRequest,
        };
        // The creator's BYOK key presence for the managed-LLM profile
        // closure (D-11 `drive_transition`).
        let key_row: Option<Option<Vec<u8>>> = sqlx::query_scalar(
            "SELECT api_key_encrypted FROM assistant_user_llm_config WHERE user_id = $1 LIMIT 1",
        )
        .bind(actor_id)
        .fetch_optional(&self.pool)
        .await?;
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
            matcher: pidash_api::v1_work_items::handlers_actions::LiveMatcher::new(
                self.pool.clone(),
            ),
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
        // Post-commit dispatch drain: unlike D-11 (whose `moved_by_run`
        // makes the outbox unreachable), human moves dispatch here —
        // after the commit, so the runner read sees the rows.
        for run_id in dispatches {
            pidash_jobs::dispatch::dispatch_agent_run(pool, &self.cloud, run_id, *now)
                .await
                .map_err(StoreError::from)?;
        }
        for consume in &deferred {
            pidash_services::dispatch::admission::consume_admission_token(&cache, consume);
        }
        if !terminal_effects.is_empty() {
            // Python's transition never finalizes a run: fail closed.
            return Err(StoreError::Transition(
                "turn transition finalized a run: unreachable, no finalization on this path"
                    .to_owned(),
            ));
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
        let key_row: Option<Option<Vec<u8>>> = sqlx::query_scalar(
            "SELECT api_key_encrypted FROM assistant_user_llm_config WHERE user_id = $1 LIMIT 1",
        )
        .bind(actor_id)
        .fetch_optional(&self.pool)
        .await?;
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
        let matcher =
            pidash_api::v1_work_items::handlers_actions::LiveMatcher::new(self.pool.clone());
        let triggered_by = pidash_db::dispatch::status::AgentRunTrigger::StateTransition.value();
        let mut tx = pidash_db::tx::Transaction::begin(&self.pool).await?;
        sqlx::query("SAVEPOINT turn_preflight_probe")
            .execute(&mut **tx.inner())
            .await?;
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
        sqlx::query("ROLLBACK TO SAVEPOINT turn_preflight_probe")
            .execute(&mut **tx.inner())
            .await?;
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
                // Post-commit dispatch drain: human moves dispatch
                // here — after the commit, so the runner read sees
                // the rows (the `drive_transition` drain shape).
                for run_id in dispatches {
                    pidash_jobs::dispatch::dispatch_agent_run(pool, &self.cloud, run_id, *now)
                        .await
                        .map_err(StoreError::from)?;
                }
                for consume in &deferred {
                    pidash_services::dispatch::admission::consume_admission_token(&cache, consume);
                }
                if !terminal_effects.is_empty() {
                    // Python's transition never finalizes a run: fail closed.
                    return Err(StoreError::Transition(
                        "turn transition finalized a run: unreachable, no finalization on this path"
                            .to_owned(),
                    ));
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
            Some(pidash_api::space::sanitize::strip_tags(&input.html))
        };
        let labels: Vec<String> = if input.fold {
            vec!["fold".to_owned()]
        } else {
            Vec::new()
        };
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO issue_comments (id, created_at, updated_at, deleted_at, issue_id, \
             project_id, workspace_id, actor_id, access, comment_json, comment_html, \
             comment_stripped, labels, speaker_type, speaker_label, external_source, \
             external_id, parent_id, attachments, description_id, edited_at, created_by_id, \
             updated_by_id) \
             VALUES ($1, $2, $3, NULL, $4, $5, $6, $7, 'INTERNAL', $8, $9, $10, $11, 'agent', \
             'Pi Dash AI', NULL, NULL, NULL, $12, NULL, NULL, $13, NULL)",
        )
        .bind(comment_id)
        .bind(now)
        .bind(now)
        .bind(input.issue_id)
        .bind(input.project_id)
        .bind(input.workspace_id)
        .bind(input.actor_id)
        .bind(Value::Object(serde_json::Map::new()))
        .bind(&input.html)
        .bind(&stripped)
        .bind(&labels)
        .bind(Value::Array(Vec::new()))
        .bind(input.actor_id)
        .execute(&mut *tx)
        .await?;
        let description_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO descriptions (id, created_at, updated_at, created_by_id, updated_by_id, \
             workspace_id, project_id, description_json, description_html, description_binary, \
             description_stripped) VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, NULL, $9)",
        )
        .bind(description_id)
        .bind(now)
        .bind(now)
        .bind(input.actor_id)
        .bind(input.workspace_id)
        .bind(input.project_id)
        .bind(Value::Object(serde_json::Map::new()))
        .bind(&input.html)
        .bind(desc_stripped)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE issue_comments SET description_id = $1 WHERE id = $2")
            .bind(description_id)
            .bind(comment_id)
            .execute(&mut *tx)
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
        let turn: Option<Uuid> = sqlx::query_scalar("SELECT id FROM assistant_turn WHERE id = $1")
            .bind(deps.turn_id)
            .fetch_optional(&self.pool)
            .await?;
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
            let locked: Option<Uuid> =
                sqlx::query_scalar("SELECT id FROM assistant_thread WHERE id = $1 FOR UPDATE")
                    .bind(deps.thread_id)
                    .fetch_optional(&mut *tx)
                    .await?;
            if locked.is_none() {
                return Err(StoreError::Missing {
                    model: "AssistantThread",
                });
            }
            let max: Option<i64> = sqlx::query_scalar(&format!(
                "SELECT MAX(m) FROM ({}) AS seqs",
                events::next_message_seq_sql()
            ))
            .bind(deps.thread_id)
            .fetch_one(&mut *tx)
            .await?;
            message_seq = max.unwrap_or(0) + 1;
            // `insert_message_sql` binds in `COLUMNS` order:
            // id, thread, turn, seq, kind, display_content, payload,
            // status, created_at, completed_at.
            sqlx::query(&events::insert_message_sql())
                .bind(message_id)
                .bind(deps.thread_id)
                .bind(turn)
                .bind(message_seq)
                .bind(pidash_services::assistant::tools_results::TOOL_RESULT_KIND)
                .bind(summary)
                .bind(&message_payload)
                .bind("completed")
                .bind(now)
                .bind(None::<DateTime<Utc>>)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
        // Event row (`events.append_event`): own transaction, thread
        // locked, thread-scoped seq; the payload embeds the envelope.
        let created_at = pidash_api::runner_enroll::enroll::render_isoformat(*now);
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
            let locked: Option<Uuid> =
                sqlx::query_scalar("SELECT id FROM assistant_thread WHERE id = $1 FOR UPDATE")
                    .bind(deps.thread_id)
                    .fetch_optional(&mut *tx)
                    .await?;
            if locked.is_none() {
                return Err(StoreError::Missing {
                    model: "AssistantThread",
                });
            }
            let max: Option<i64> = sqlx::query_scalar(&format!(
                "SELECT MAX(m) FROM ({}) AS seqs",
                events::next_event_seq_sql()
            ))
            .bind(deps.thread_id)
            .fetch_one(&mut *tx)
            .await?;
            event_seq = max.unwrap_or(0) + 1;
            // `insert_event_sql` binds in `COLUMNS` order minus the pk:
            // thread, turn, seq, kind, message, payload, created_at.
            sqlx::query(&events::insert_event_sql())
                .bind(deps.thread_id)
                .bind(turn)
                .bind(event_seq)
                .bind(pidash_services::assistant::tools_results::TOOL_RESULT_KIND)
                .bind(message_id)
                .bind(&event_payload)
                .bind(now)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
        // Best-effort publish (`events.publish_event` swallows every
        // failure, logging and returning). The event id for the frame
        // is re-read (the builder has no `RETURNING`); the payload
        // renders spaced (`json.dumps`), not compact.
        if let Some(redis) = &self.redis {
            let event_id: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM assistant_event WHERE thread_id = $1 AND seq = $2",
            )
            .bind(deps.thread_id)
            .bind(event_seq)
            .fetch_optional(&self.pool)
            .await?;
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
        let config: Option<Value> =
            sqlx::query_scalar(pidash_services::assistant::tools_github::BINDING_TOKEN_SQL)
                .bind(deps.user_id)
                .bind(&deps.workspace_slug)
                .bind(provider)
                .bind(namespace)
                .bind(repo)
                .fetch_optional(&self.pool)
                .await?;
        Ok(config)
    }

    async fn github_sync_credentials(
        &self,
        deps: &AssistantDeps,
        namespace: &str,
        repo: &str,
    ) -> Result<Option<Value>, StoreError> {
        let credentials: Option<Value> =
            sqlx::query_scalar(pidash_services::assistant::tools_github::SYNC_TOKEN_SQL)
                .bind(deps.user_id)
                .bind(&deps.workspace_slug)
                .bind(namespace)
                .bind(repo)
                .fetch_optional(&self.pool)
                .await?;
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
    matcher: pidash_api::v1_work_items::handlers_actions::LiveMatcher,
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
    use pidash_services::assistant::tools_issues::{MEMBER_PROJECTS_SQL, SCOPED_ISSUES_SQL};

    fn squashed(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
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
            max_per_user_project: 1,
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
             default_agent_executor, workspace_id) \
             VALUES ($1, $2, $2, 'T1124', $3, '', 2, false, false, false, true, false, false, \
             false, false, false, true, 0, 0, '{}', 'UTC', 'https://example.com/t1124.git', \
             'main', 10800, 10, 10800, 10800, true, 'local_runner', $4)",
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

    /// The inlined FROM/WHERE fragments equal the reviewed services
    /// consts (the row readers only swap the SELECT list).
    #[test]
    fn sql_fragments_match_services_consts() {
        assert_eq!(
            squashed(&format!("SELECT DISTINCT issues.* {SCOPED_ISSUES_FILTER}")),
            squashed(SCOPED_ISSUES_SQL),
            "scoped issues fragment"
        );
        assert_eq!(
            squashed(&format!(
                "SELECT DISTINCT projects.id {MEMBER_PROJECTS_FILTER}"
            )),
            squashed(MEMBER_PROJECTS_SQL),
            "member projects fragment"
        );
    }
}
