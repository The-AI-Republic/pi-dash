//! D-18 permission guards (stage 5, PIDASHCONV-671).
//!
//! Ports the `api/views/issue.py` guard closure and the per-endpoint
//! permission wiring for the api layer. Fixture:
//! `rust-api/fixtures/v1_work_items/guards/F18-09.guards.json` (trace:
//! `rust-api/fixtures/v1_work_items/TRACE.md`, `## Guards`).
//!
//! Shape of the port (the merged D-19 `v1_projects/perms.rs` pattern): the
//! decision kernels live in the read-only F-06 foundation
//! (`pidash_auth::permissions::project`); this module only pins which gate
//! each D-18 route+method carries ([`gate_for`]), decides through the
//! kernel ([`decide`]), and pins the byte-exact guard bodies the handlers
//! answer. Row fetching stays with the handler layer (PIDASHCONV-673…680),
//! which must honor the fetch-scoping traps documented on each function.
//!
//! Gate order (preserved, not redesigned): DRF `initial()` runs API-key
//! authentication (`api/views/base.py:101`), then the slug→UUID rewrite
//! (`base.py:52-104`, skipped for anonymous callers so slugs cannot be
//! probed via 404-vs-401), then `check_permissions`, then the handler body.
//! Anonymous callers therefore 401 on every D-18 route and never reach a
//! gate (fixture `anon_list`).
//!
//! Response shapes (handlers render these; recorded here so the matrix has
//! one home):
//!
//! * Anonymous on any D-18 route: 401 [`UNAUTHENTICATED_BODY`] — the auth
//!   layer answers before any gate runs. An invalid token answers 403
//!   `{"detail":"Given API token is not valid"}` (pinned by the auth
//!   layer, outside this port).
//! * Denied member: 403 [`CLASS_DENIAL_BODY`] — the DRF-default
//!   `PermissionDenied` body. None of the 3 classes sets `message`, so
//!   every class denial renders through `APIView.permission_denied` with
//!   `message=None`.
//! * `IsAuthenticated` routes (yield declared, `views/issue.py:1266`;
//!   attachments / search inherited from `BaseAPIView`,
//!   `views/base.py:103`) carry [`V1WorkItemsGate::AuthOnly`]: any
//!   authenticated caller passes the gate; the attachment endpoints then
//!   apply [`user_has_issue_permission`] inside their bodies.
//!
//! Throttles (verified, not ported): neither `api/views/issue.py`,
//! `api/views/page.py`, `api/views/github_pr.py` nor
//! `api/views/git_code_review.py` declares `throttle_classes`,
//! `throttle_scope`, or any `throttle` reference — rate limiting on these
//! routes comes only from the shared `BaseAPIView.get_throttles`
//! (`ApiKeyRateThrottle` / `ServiceTokenRateThrottle`,
//! `api/views/base.py:118-131`), which is cross-cutting infrastructure
//! outside this guard port.
//!
//! Name resolution (verified against the imports): `views/issue.py:98`
//! imports `ROLE` from `pi_dash.app.permissions`; the guard classes come
//! from the same `app/permissions/project.py:56-143` the F-06 kernel
//! ports, so no `utils/`-copy divergence applies to D-18.
//!
//! Ported quirks (translate, don't redesign):
//!
//! * BUG (fixture note, TRACE-confirmed) [`refuse_agent_action`] ignores
//!   run ownership: ANY active run on the issue named by the header
//!   refuses — even a run the caller may not speak for (fixture
//!   `foreign_active_on_issue_refused`).
//! * BUG-7 (kernel) `ProjectMemberPermission` SAFE is workspace-scoped,
//!   not project-scoped (`project.py:62-65`: no `project_id` filter) —
//!   any active project membership in the workspace reads every label
//!   list. Handlers must fetch that one fact without a `project_id`
//!   filter; see [`decide`].
//! * The epic text counts 38 `work_item` routes; `api/urls/work_item.py`
//!   holds 39 (12 deprecated `issues/` twins + 27 `work-items/`). The
//!   table below pins all 39 plus the 2 label and 3 page routes.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52` (zero drift
//! Ported-from→HEAD on all D-18 guard and view sources, verified
//! 2026-10-02 by PIDASHCONV-659).

use pidash_auth::permissions::project;
use pidash_auth::scope::TenantScope;
use pidash_types::runner_runs::AgentRunStatus;
use uuid::Uuid;

/// The DRF-default permission-denied body every D-18 class denial renders.
///
/// Byte-exact: `{"detail":"You do not have permission to perform this action."}`
/// (compact separators, `rest_framework` defaults). Alias of
/// [`crate::permissions::DEFAULT_DENIED_BODY`] so handlers have one home.
pub const CLASS_DENIAL_BODY: &str = crate::permissions::DEFAULT_DENIED_BODY;

/// Exact bytes of the anonymous denial on every D-18 route (fixture
/// `anon_list`): the API-key authentication layer answers 401 before any
/// gate runs.
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;

/// The agent-run header (`views/issue.py:1067`) every guard below reads.
pub const RUN_ID_HEADER: &str = "X-Pi-Dash-Run-Id";

/// [`ResolveRunError::NotUuid`] rendered as the move-PATCH 400 body
/// (`views/issue.py:802-804`).
pub const HEADER_NOT_UUID_BODY: &str = r#"{"error":"X-Pi-Dash-Run-Id is not a UUID"}"#;
/// [`ResolveRunError::NotYours`] rendered as the move-PATCH 400 body.
pub const HEADER_NOT_YOURS_BODY: &str =
    r#"{"error":"X-Pi-Dash-Run-Id names a run that is not yours"}"#;

/// Work-item delete 403 (`views/issue.py:894-897`).
pub const DELETE_DENIAL_BODY: &str =
    r#"{"error":"Only admin or creator can delete the work item"}"#;
/// Attachment upload / upload-confirm 403 (`views/issue.py:2327-2328`,
/// `:2621-2624`).
pub const ATTACHMENT_UPLOAD_DENIAL_BODY: &str =
    r#"{"error":"You are not allowed to upload this attachment"}"#;
/// Attachment delete 403 (`views/issue.py:2483-2486`).
pub const ATTACHMENT_DELETE_DENIAL_BODY: &str =
    r#"{"error":"You are not allowed to delete this attachment"}"#;
/// Attachment download 403 (`views/issue.py:2556-2559`).
pub const ATTACHMENT_DOWNLOAD_DENIAL_BODY: &str =
    r#"{"error":"You are not allowed to download this attachment"}"#;
/// Locked-page 409 (`views/page.py:491-493`; key order `error`,
/// `error_code`, `error_message` as built by `_error`).
pub const PAGE_LOCKED_BODY: &str =
    r#"{"error":"Page is locked","error_code":4701,"error_message":"PAGE_LOCKED"}"#;
/// Non-owner non-admin archive 403 (`views/page.py:497-501`).
pub const PAGE_OWNER_DENIAL_BODY: &str =
    r#"{"error":"Only the page owner or a project admin can archive or unarchive it"}"#;

/// `ERROR_CODES["PAGE_LOCKED"]` (`utils/error_codes.py:12`).
pub const PAGE_LOCKED_CODE: u32 = 4701;

/// Which permission class (or lack of one) guards a D-18 route+method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V1WorkItemsGate {
    /// `ProjectEntityPermission` (`app/permissions/project.py:85-116`).
    ProjectEntity,
    /// `ProjectLitePermission` (`project.py:133-143`).
    ProjectLite,
    /// `ProjectMemberPermission` (`project.py:56-82`).
    ProjectMember,
    /// `IsAuthenticated` only: declared on `AgentRunYieldAPIEndpoint`
    /// (`views/issue.py:1266`), inherited from `BaseAPIView`
    /// (`views/base.py:103`) on the attachment and search endpoints.
    AuthOnly,
}

/// One D-18 URL pattern (methods vary per pattern; see `api/urls/`).
///
/// Deprecated `issues/` twins share their view class with the `work-items/`
/// route, so they share its gate — the twins are separate variants only so
/// the table pins that both spellings are wired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V1WorkItemsRoute {
    // -- Deprecated `issues/` twins (`api/urls/work_item.py:39-98`). --
    /// `workspaces/<slug>/issues/search/` (`urls/work_item.py:39`, GET):
    /// `IssueSearchEndpoint` (`views/issue.py:2654`).
    OldSearch,
    /// `workspaces/<slug>/issues/<project>-<issue>/` (`:44`, GET):
    /// `WorkspaceIssueAPIEndpoint` (`views/issue.py:190`).
    OldByIdentifier,
    /// `.../projects/<id>/issues/` (`:49`, GET/POST):
    /// `IssueListCreateAPIEndpoint` (`views/issue.py:272`).
    OldIssueList,
    /// `.../issues/<pk>/` (`:54`, GET/PATCH/DELETE):
    /// `IssueDetailAPIEndpoint` (`views/issue.py:543`).
    OldIssueDetail,
    /// `.../issues/<id>/links/` (`:59`, GET/POST):
    /// `IssueLinkListCreateAPIEndpoint` (`views/issue.py:1549`).
    OldLinkList,
    /// `.../links/<pk>/` (`:64`, GET/PATCH/DELETE):
    /// `IssueLinkDetailAPIEndpoint` (`views/issue.py:1653`).
    OldLinkDetail,
    /// `.../issues/<id>/comments/` (`:69`, GET/POST):
    /// `IssueCommentListCreateAPIEndpoint` (`views/issue.py:1796`).
    OldCommentList,
    /// `.../comments/<pk>/` (`:74`, GET/PATCH/DELETE):
    /// `IssueCommentDetailAPIEndpoint` (`views/issue.py:1952`).
    OldCommentDetail,
    /// `.../issues/<id>/activities/` (`:79`, GET):
    /// `IssueActivityListAPIEndpoint` (`views/issue.py:2124`).
    OldActivityList,
    /// `.../activities/<pk>/` (`:84`, GET):
    /// `IssueActivityDetailAPIEndpoint` (`views/issue.py:2176`).
    OldActivityDetail,
    /// `.../issues/<id>/issue-attachments/` (`:89`, GET/POST):
    /// `IssueAttachmentListCreateAPIEndpoint` (`views/issue.py:2235`).
    OldAttachmentList,
    /// `.../issue-attachments/<pk>/` (`:94`, GET/PATCH/DELETE):
    /// `IssueAttachmentDetailAPIEndpoint` (`views/issue.py:2450`).
    OldAttachmentDetail,
    // -- `work-items/` routes (`api/urls/work_item.py:103-237`). --
    /// `workspaces/<slug>/work-items/search/` (`:103`, GET):
    /// `IssueSearchEndpoint` (`views/issue.py:2654`).
    Search,
    /// `workspaces/<slug>/work-items/search/advanced/` (`:108`, GET):
    /// `IssueAdvancedSearchEndpoint` (`views/issue.py:2723`).
    SearchAdvanced,
    /// `workspaces/<slug>/work-items/<project>-<issue>/` (`:113`, GET):
    /// `WorkspaceIssueAPIEndpoint` (`views/issue.py:190`).
    ByIdentifier,
    /// `.../projects/<id>/work-items/` (`:118`, GET/POST):
    /// `IssueListCreateAPIEndpoint` (`views/issue.py:272`).
    IssueList,
    /// `.../work-items/<pk>/` (`:123`, GET/PATCH/DELETE):
    /// `IssueDetailAPIEndpoint` (`views/issue.py:543`).
    IssueDetail,
    /// `.../work-items/<pk>/move/` (`:128`, POST):
    /// `IssueMoveAPIEndpoint` (`views/issue.py:912`).
    IssueMove,
    /// `.../work-items/<pk>/re-tick/` (`:133`, POST):
    /// `IssueReTickAPIEndpoint` (`views/issue.py:940`).
    IssueReTick,
    /// `.../work-items/<pk>/wait/` (`:138`, POST):
    /// `IssueWaitAPIEndpoint` (`views/issue.py:987`).
    IssueWait,
    /// `.../work-items/<pk>/run-ai/` (`:143`, POST):
    /// `IssueRunAiAPIEndpoint` (`views/issue.py:1175`).
    IssueRunAi,
    /// `workspaces/<slug>/agent-runs/<run_id>/yield/` (`:148`, POST):
    /// `AgentRunYieldAPIEndpoint` (`views/issue.py:1255`).
    AgentRunYield,
    /// `.../work-items/<id>/links/` (`:153`, GET/POST):
    /// `IssueLinkListCreateAPIEndpoint` (`views/issue.py:1549`).
    LinkList,
    /// `.../links/<pk>/` (`:158`, GET/PATCH/DELETE):
    /// `IssueLinkDetailAPIEndpoint` (`views/issue.py:1653`).
    LinkDetail,
    /// `.../work-items/<id>/comments/` (`:163`, GET/POST):
    /// `IssueCommentListCreateAPIEndpoint` (`views/issue.py:1796`).
    CommentList,
    /// `.../comments/<pk>/` (`:168`, GET/PATCH/DELETE):
    /// `IssueCommentDetailAPIEndpoint` (`views/issue.py:1952`).
    CommentDetail,
    /// `.../work-items/<id>/activities/` (`:173`, GET):
    /// `IssueActivityListAPIEndpoint` (`views/issue.py:2124`).
    ActivityList,
    /// `.../activities/<pk>/` (`:178`, GET):
    /// `IssueActivityDetailAPIEndpoint` (`views/issue.py:2176`).
    ActivityDetail,
    /// `.../work-items/<id>/attachments/` (`:183`, GET/POST):
    /// `IssueAttachmentListCreateAPIEndpoint` (`views/issue.py:2235`).
    AttachmentList,
    /// `.../attachments/<pk>/` (`:188`, GET/PATCH/DELETE):
    /// `IssueAttachmentDetailAPIEndpoint` (`views/issue.py:2450`).
    AttachmentDetail,
    /// `.../work-items/<id>/relations/` (`:193`, GET/POST):
    /// `IssueRelationListCreateAPIEndpoint` (`views/issue.py:2918`).
    RelationList,
    /// `.../relations/grouped/` (`:198`, GET):
    /// `IssueRelationGroupedAPIEndpoint` (`views/issue.py:3197`,
    /// gate inherited from `_IssueRelationAgentBase`, `:3151`).
    RelationGrouped,
    /// `.../relations/relate/` (`:203`, POST):
    /// `IssueRelationRelateAPIEndpoint` (`views/issue.py:3220`).
    RelationRelate,
    /// `.../relations/unrelate/` (`:208`, POST):
    /// `IssueRelationUnrelateAPIEndpoint` (`views/issue.py:3238`).
    RelationUnrelate,
    /// `.../work-items/<id>/workpad/` (`:213`, GET/PATCH):
    /// `IssueWorkpadAPIEndpoint` (`views/issue.py:3256`).
    Workpad,
    /// `.../work-items/<id>/github/pull-requests/` (`:218`, GET/POST):
    /// `GithubPullRequestLinkListCreateAPIEndpoint`
    /// (`views/github_pr.py:29`).
    GithubPrList,
    /// `.../pull-requests/<pk>/` (`:223`, DELETE):
    /// `GithubPullRequestLinkDetailAPIEndpoint`
    /// (`views/github_pr.py:79`).
    GithubPrDetail,
    /// `.../work-items/<id>/code-reviews/` (`:228`, GET/POST):
    /// `GitCodeReviewLinkListCreateAPIEndpoint`
    /// (`views/git_code_review.py:24`).
    CodeReviewList,
    /// `.../code-reviews/<pk>/` (`:233`, DELETE):
    /// `GitCodeReviewLinkDetailAPIEndpoint`
    /// (`views/git_code_review.py:79`).
    CodeReviewDetail,
    // -- Label routes (`api/urls/label.py`). --
    /// `.../projects/<id>/labels/` (`urls/label.py:11`, GET/POST):
    /// `LabelListCreateAPIEndpoint` (`views/issue.py:1314`).
    LabelList,
    /// `.../labels/<pk>/` (`urls/label.py:16`, GET/PATCH/DELETE):
    /// `LabelDetailAPIEndpoint` (`views/issue.py:1440`).
    LabelDetail,
    // -- Page routes (`api/urls/page.py`). --
    /// `.../projects/<id>/pages/` (`urls/page.py:14`, GET/POST):
    /// `PageListAPIEndpoint` (`views/page.py:233`, gate inherited from
    /// `BasePageReadAPIEndpoint`, `:177`).
    PageList,
    /// `.../pages/<page_id>/` (`urls/page.py:19`, GET/PATCH):
    /// `PageDetailAPIEndpoint` (`views/page.py:344`).
    PageDetail,
    /// `.../pages/<page_id>/archive/` (`urls/page.py:24`, POST/DELETE):
    /// `PageArchiveAPIEndpoint` (`views/page.py:480`).
    PageArchive,
}

/// Map a D-18 route+method to its gate, exactly as the view classes declare.
///
/// No D-18 view class overrides `get_permissions`, so the gate never
/// branches on the method — `method` is accepted only to keep the D-19
/// [`gate_for`](crate::v1_projects::perms::gate_for) call shape. The
/// kernel ([`decide`]) still branches on it internally (SAFE vs POST vs
/// other unsafe) exactly as the classes do.
pub fn gate_for(route: V1WorkItemsRoute, _method: &str) -> V1WorkItemsGate {
    use V1WorkItemsGate as G;
    use V1WorkItemsRoute as R;
    match route {
        R::OldCommentList | R::OldCommentDetail | R::CommentList | R::CommentDetail => {
            G::ProjectLite
        }
        R::LabelList | R::LabelDetail => G::ProjectMember,
        R::OldSearch
        | R::OldAttachmentList
        | R::OldAttachmentDetail
        | R::Search
        | R::SearchAdvanced
        | R::AgentRunYield
        | R::AttachmentList
        | R::AttachmentDetail => G::AuthOnly,
        _ => G::ProjectEntity,
    }
}

/// Decide a gate from caller-fetched membership facts.
///
/// Each boolean mirrors one `...objects.filter(...).exists()` in the guard
/// class; the caller's SQL carries the filters shown. `true` passes the
/// request into the handler body, `false` answers 403
/// [`CLASS_DENIAL_BODY`]. Anonymous callers never reach here (the auth layer
/// 401s first); the kernel's `authenticated` flag is set, not checked, by
/// handlers — it exists so a handler cannot accidentally authorize a
/// caller it failed to authenticate.
///
/// Fetch-scoping traps the handler layer must honor (the decision functions
/// themselves are scope-correct only when the facts are fetched as below):
///
/// * `ProjectMemberPermission` SAFE (label list/detail reads) uses the
///   **workspace-scoped** `is_project_member`: `ProjectMember` filtered by
///   `(workspace__slug, member, is_active)` with **no** `project_id`
///   (BUG-7, `project.py:62-65`). Every other project fact on every other
///   gate adds `project_id=view.project_id`.
/// * `ProjectEntityPermission` SAFE on the by-identifier routes
///   ([`V1WorkItemsRoute::ByIdentifier`] /
///   [`V1WorkItemsRoute::OldByIdentifier`]) reads
///   `has_identifier_membership` from the identifier's project, not
///   `view.project_id` (`project.py:91-98`): `WorkspaceIssueAPIEndpoint`
///   is the only D-18 class with a `project_identifier` property
///   (`views/issue.py:202-204`).
/// * `ProjectMemberPermission` POST (label create) reads
///   `has_workspace_admin_or_member` from `WorkspaceMember`, not
///   `ProjectMember` (`project.py:67-73`).
pub fn decide(
    gate: V1WorkItemsGate,
    method: &str,
    scope: &TenantScope,
    facts: &project::ProjectFacts,
) -> bool {
    match gate {
        V1WorkItemsGate::ProjectEntity => project::decide_project_entity(method, scope, facts),
        V1WorkItemsGate::ProjectLite => project::decide_project_lite(scope, facts),
        V1WorkItemsGate::ProjectMember => project::decide_project_member(method, scope, facts),
        // The base `IsAuthenticated` already passed, so the authenticated
        // caller reaches the handler body.
        V1WorkItemsGate::AuthOnly => true,
    }
}

/// `user_has_issue_permission` (`views/issue.py:175-189`).
///
/// `issue_created_by_id` is `Some` exactly when the caller passes an
/// issue (`None` on the attachment-download path); `membership_exists` is
/// the single trailing `qs.exists()` with the caller's SQL carrying the
/// filters — always `(project_id, member_id, is_active)`, plus
/// `role__in=allowed_roles` when the call site passes roles. `allow_creator`
/// short-circuits before any DB check.
///
/// Call sites and their SQL (handlers fetch; this function decides):
///
/// * Upload (`:2320-2326`) and upload-confirm (`:2614-2620`):
///   `allowed_roles=[ADMIN, MEMBER, GUEST]`, `allow_creator=True`; denial
///   403 [`ATTACHMENT_UPLOAD_DENIAL_BODY`].
/// * Delete attachment (`:2476-2482`): same roles, `allow_creator=True`;
///   denial 403 [`ATTACHMENT_DELETE_DENIAL_BODY`].
/// * Download (`:2549-2555`): `issue=None`, `allowed_roles=None`,
///   `allow_creator=False` — any active project membership; denial 403
///   [`ATTACHMENT_DOWNLOAD_DENIAL_BODY`].
pub fn user_has_issue_permission(
    user_id: Uuid,
    issue_created_by_id: Option<Uuid>,
    membership_exists: bool,
    allow_creator: bool,
) -> bool {
    if allow_creator && issue_created_by_id == Some(user_id) {
        return true;
    }
    membership_exists
}

/// One `AgentRun` row as the agent-guard closure reads it.
///
/// Column sources (`runner/models.py`): `id` (`:873`), `created_by`
/// (`:891-896`, non-null), `owner` (`:882-888`, nullable),
/// `runner` (`:903-909`, nullable), `work_item` (`:922-928`, nullable),
/// `status` (`:947-952`).
///
/// Fetch: the header paths (`resolve_moved_by_run`, `refuse_agent_action`)
/// read the row by primary key; the no-header inference path
/// ([`active_run_of_caller`]) reads this issue's rows newest-first.
/// `runner_owner_id` needs the `select_related("runner")` join the
/// Python paths take (`views/issue.py:1113,1127`); it is `None` when
/// `runner_id` is `None` (or the runner's owner is null).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunFacts {
    /// `AgentRun.id`.
    pub id: Uuid,
    /// `AgentRun.created_by_id` (non-null).
    pub created_by_id: Uuid,
    /// `AgentRun.owner_id` (nullable).
    pub owner_id: Option<Uuid>,
    /// `AgentRun.runner_id` (nullable).
    pub runner_id: Option<Uuid>,
    /// `AgentRun.runner.owner_id` via the runner join.
    pub runner_owner_id: Option<Uuid>,
    /// `AgentRun.work_item_id` (nullable).
    pub work_item_id: Option<Uuid>,
    /// `AgentRun.status`, mapped with
    /// [`AgentRunStatus::from_value`]. Python's `is_active` membership
    /// test is `False` for any stored value outside the active set, so
    /// handlers map an unknown stored value to any non-active member
    /// (e.g. `Cancelled`) — never to an active one.
    pub status: AgentRunStatus,
}

/// `run_belongs_to` (`views/issue.py:1070-1085`): may `user_id` speak for
/// `run` — they created it, own it, or own the runner it executes on?
/// Workspace membership alone is not enough.
///
/// `None` user (anonymous) or `None` run denies, as the `is None` guard
/// does. Direct call site besides the closure: the yield endpoint renders
/// a foreign run as 404 `{"error":"run not found"}`
/// (`views/issue.py:1294-1296`).
pub fn run_belongs_to(user_id: Option<Uuid>, run: Option<&RunFacts>) -> bool {
    let (Some(user_id), Some(run)) = (user_id, run) else {
        return false;
    };
    if run.created_by_id == user_id || run.owner_id == Some(user_id) {
        return true;
    }
    run.runner_id.is_some() && run.runner_owner_id == Some(user_id)
}

/// `resolve_moved_by_run` failure (`views/issue.py:1108-1117`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveRunError {
    /// The header value is not a UUID (`:1111`).
    NotUuid,
    /// The header names a run the caller may not speak for (`:1117`).
    NotYours,
}

impl ResolveRunError {
    /// The exact Python message (`f"{RUN_ID_HEADER} …"`).
    pub fn message(self) -> &'static str {
        match self {
            ResolveRunError::NotUuid => "X-Pi-Dash-Run-Id is not a UUID",
            ResolveRunError::NotYours => "X-Pi-Dash-Run-Id names a run that is not yours",
        }
    }

    /// The message rendered as the move-PATCH 400 body
    /// (`views/issue.py:802-804`).
    pub fn body(self) -> &'static str {
        match self {
            ResolveRunError::NotUuid => HEADER_NOT_UUID_BODY,
            ResolveRunError::NotYours => HEADER_NOT_YOURS_BODY,
        }
    }
}

/// `resolve_moved_by_run` (`views/issue.py:1086-1122`): `(run, error)` for
/// the [`RUN_ID_HEADER`] header.
///
/// * No (or blank) header: infer the caller's active run on this issue via
///   [`active_run_of_caller`] over `newest_runs_on_issue` — never an error.
/// * Unparseable header: `(None, NotUuid)`. `Uuid::parse_str` accepts the
///   same four spellings `uuid.UUID` does (simple, hyphenated, braced,
///   `urn:uuid:`). CPython's parser is sloppier in corners no real caller
///   sends (misplaced hyphens, unbalanced braces, `_` separators), which
///   answer `NotUuid` here; the strict parse stands.
/// * Unknown id, or an owned run on another issue / a finished run:
///   `(None, None)` — "simply not an agent move on this issue".
/// * A run the caller may not speak for: `(None, NotYours)` — checked
///   BEFORE the work-item/active filter, so a foreign run anywhere
///   errors rather than passing silently.
///
/// `run_by_id` is the row fetched by the parsed header id (`:1113`), or
/// `None` when the lookup finds nothing; a row whose id differs from the
/// parsed header is treated as not found. Returns the resolved run's id —
/// handlers already hold the fetched rows, so no refetch is needed.
///
/// Rendering: the move PATCH answers the error as 400 `{"error": msg}`
/// (`:802-804`); the wait endpoint ignores it (best-effort attribution,
/// `:1044-1047`).
pub fn resolve_moved_by_run(
    header: Option<&str>,
    user_id: Option<Uuid>,
    issue_id: Uuid,
    run_by_id: Option<&RunFacts>,
    newest_runs_on_issue: &[RunFacts],
) -> (Option<Uuid>, Option<ResolveRunError>) {
    let raw = header.unwrap_or("").trim();
    if raw.is_empty() {
        return (
            active_run_of_caller(user_id, issue_id, newest_runs_on_issue),
            None,
        );
    }
    let run_id = match Uuid::parse_str(raw) {
        Ok(run_id) => run_id,
        Err(_) => return (None, Some(ResolveRunError::NotUuid)),
    };
    let run = match run_by_id {
        Some(run) if run.id == run_id => run,
        _ => return (None, None),
    };
    if !run_belongs_to(user_id, Some(run)) {
        return (None, Some(ResolveRunError::NotYours));
    }
    if run.work_item_id != Some(issue_id) || !run.status.is_active() {
        return (None, None);
    }
    (Some(run.id), None)
}

/// `_active_run_of_caller` (`views/issue.py:1123-1132`): the active run on
/// `issue_id` the caller may speak for, if any.
///
/// `newest_runs_first` is the handler's fetch — this issue's runs ordered
/// by `-created_at` (`:1127`) — and only its first 5 rows are considered,
/// exactly as the Python `[:5]` slice. The work-item equality is
/// re-checked per row so a sloppy input cannot resolve another issue's
/// run; with the documented fetch it never filters anything out.
pub fn active_run_of_caller(
    user_id: Option<Uuid>,
    issue_id: Uuid,
    newest_runs_first: &[RunFacts],
) -> Option<Uuid> {
    newest_runs_first
        .iter()
        .take(5)
        .filter(|run| run.work_item_id == Some(issue_id))
        .find(|run| run.status.is_active() && run_belongs_to(user_id, Some(run)))
        .map(|run| run.id)
}

/// A refused human-only lever: always 403 with
/// `{"error":"{action} is a human action; it cannot be requested from inside
/// an agent run"}` (`views/issue.py:1155-1158`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentActionRefusal {
    /// Always 403 (`status.HTTP_403_FORBIDDEN`).
    pub status: u16,
    /// The exact JSON body.
    pub body: String,
}

/// `{"error":"{action} is a human action; it cannot be requested from inside
/// an agent run"}` in compact DRF rendering.
pub fn refusal_body(action: &str) -> String {
    serde_json::json!({
        "error": format!("{action} is a human action; it cannot be requested from inside an agent run"),
    })
    .to_string()
}

/// `_refuse_agent_action` (`views/issue.py:1133-1161`): refuse a human-only
/// lever requested from inside an agent run.
///
/// Refuses (403) exactly when the header parses as a UUID AND names a run
/// that is active on THIS issue. A missing/blank header, a malformed id,
/// an unknown id, a run on another issue, or a finished run is allowed
/// through (`None`).
///
/// BUG PORTED (`:1153-1154`, fixture `foreign_active_on_issue_refused`):
/// the lookup filters by `(pk, work_item_id)` only — run ownership is
/// never checked, so a foreign active run on this issue still refuses.
///
/// `run_by_id` is the row fetched by the parsed header id, or `None` when
/// the lookup finds nothing; a row whose id differs from the parsed
/// header is treated as not found. The fetch needs only
/// `(id, work_item_id, status)` — no runner join.
pub fn refuse_agent_action(
    header: Option<&str>,
    issue_id: Uuid,
    run_by_id: Option<&RunFacts>,
    action: &str,
) -> Option<AgentActionRefusal> {
    let raw = header.unwrap_or("").trim();
    if raw.is_empty() {
        return None;
    }
    let run_id = Uuid::parse_str(raw).ok()?;
    let run = run_by_id.filter(|run| run.id == run_id)?;
    if run.work_item_id == Some(issue_id) && run.status.is_active() {
        return Some(AgentActionRefusal {
            status: 403,
            body: refusal_body(action),
        });
    }
    None
}

/// `_refuse_agent_retick` (`views/issue.py:1162-1164`): the re-tick alias
/// with `action="re-tick"`. (Run AI calls [`refuse_agent_action`] with
/// `action="Run AI"`, `views/issue.py:1232`.)
pub fn refuse_agent_retick(
    header: Option<&str>,
    issue_id: Uuid,
    run_by_id: Option<&RunFacts>,
) -> Option<AgentActionRefusal> {
    refuse_agent_action(header, issue_id, run_by_id, "re-tick")
}

/// Work-item delete guard (`views/issue.py:885-897`): the creator or a
/// project admin may delete; anyone else answers 403
/// [`DELETE_DENIAL_BODY`]. A guest can therefore delete exactly the items
/// they created — the guest `created_by` scoping.
///
/// `is_project_admin` is `ProjectMember.objects.filter(workspace__slug,
/// member, role=20, project_id, is_active).exists()` — note the hardcoded
/// `role=20` (`ROLE.ADMIN`) and the workspace-slug scope, unlike the
/// page-archive admin check below.
pub fn can_delete_work_item(
    user_id: Uuid,
    issue_created_by_id: Uuid,
    is_project_admin: bool,
) -> bool {
    user_id == issue_created_by_id || is_project_admin
}

/// `_check_can_archive` outcome (`views/page.py:491-502`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveDecision {
    /// May archive / unarchive.
    Allow,
    /// The page is locked: 409 [`PAGE_LOCKED_BODY`].
    Locked,
    /// Neither owner nor project admin: 403 [`PAGE_OWNER_DENIAL_BODY`].
    Forbidden,
}

impl ArchiveDecision {
    /// The HTTP status the handler answers (`None` on [`ArchiveDecision::Allow`]).
    pub fn status(self) -> Option<u16> {
        match self {
            ArchiveDecision::Allow => None,
            ArchiveDecision::Locked => Some(409),
            ArchiveDecision::Forbidden => Some(403),
        }
    }

    /// The exact JSON body the handler answers (`None` on [`ArchiveDecision::Allow`]).
    pub fn body(self) -> Option<&'static str> {
        match self {
            ArchiveDecision::Allow => None,
            ArchiveDecision::Locked => Some(PAGE_LOCKED_BODY),
            ArchiveDecision::Forbidden => Some(PAGE_OWNER_DENIAL_BODY),
        }
    }
}

/// `_check_can_archive` (`views/page.py:491-502`).
///
/// The lock check runs FIRST: a locked page answers 409 even to its owner.
/// Otherwise the page owner or a project admin passes; anyone else —
/// including a project MEMBER — answers 403. `is_project_admin` is
/// `ProjectMember.objects.filter(project_id, member, is_active,
/// role=ADMIN).exists()` — note: NO `workspace__slug` filter, unlike the
/// delete guard above.
///
/// Ordering note (fixture): a guest POST never reaches this function —
/// `ProjectEntityPermission` (page routes carry Entity via
/// `BasePageReadAPIEndpoint`, `views/page.py:177`) refuses guests on
/// unsafe methods first with 403 [`CLASS_DENIAL_BODY`].
pub fn check_can_archive(
    is_locked: bool,
    page_owned_by_id: Uuid,
    user_id: Uuid,
    is_project_admin: bool,
) -> ArchiveDecision {
    if is_locked {
        return ArchiveDecision::Locked;
    }
    if page_owned_by_id == user_id || is_project_admin {
        return ArchiveDecision::Allow;
    }
    ArchiveDecision::Forbidden
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_types::{ProjectId, WorkspaceId};
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/v1_work_items/guards/F18-09.guards.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("F18-09 fixture exists"))
            .expect("F18-09 parses")
    }

    fn matrix(fx: &Value, unit: &str) -> Value {
        fx["units"][unit]["matrix"].clone()
    }

    /// Guard-produced JSON bodies compare as parsed values (key order is
    /// pinned separately by the byte-exact const assertions).
    fn body_is(actual: &str, expected: &Value) {
        let parsed: Value = serde_json::from_str(actual).expect("body parses");
        assert_eq!(&parsed, expected);
    }

    fn scope() -> TenantScope {
        TenantScope::new(WorkspaceId::from("acme"))
    }

    fn project_facts() -> project::ProjectFacts {
        project::ProjectFacts {
            workspace: WorkspaceId::from("acme"),
            project_id: ProjectId::from("p-1"),
            authenticated: true,
            is_workspace_member: false,
            has_workspace_admin_or_member: false,
            is_workspace_admin: false,
            is_project_member: false,
            is_project_admin: false,
            has_project_admin_or_member: false,
            has_identifier_membership: false,
            has_project_identifier: false,
        }
    }

    /// Active project GUEST row (plus workspace membership).
    fn project_guest(pf: &mut project::ProjectFacts) {
        pf.is_workspace_member = true;
        pf.has_workspace_admin_or_member = true;
        pf.is_project_member = true;
    }

    /// Active project MEMBER row (plus workspace membership).
    fn project_member(pf: &mut project::ProjectFacts) {
        pf.is_workspace_member = true;
        pf.has_workspace_admin_or_member = true;
        pf.is_project_member = true;
        pf.has_project_admin_or_member = true;
    }

    fn user() -> Uuid {
        Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap()
    }

    fn stranger() -> Uuid {
        Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap()
    }

    fn issue() -> Uuid {
        Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap()
    }

    fn other_issue() -> Uuid {
        Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap()
    }

    /// `RunFacts` for `id`, created by `user`, active on `issue`.
    fn own_run(id: &str, user_id: Uuid, issue_id: Uuid) -> RunFacts {
        RunFacts {
            id: Uuid::parse_str(id).unwrap(),
            created_by_id: user_id,
            owner_id: None,
            runner_id: None,
            runner_owner_id: None,
            work_item_id: Some(issue_id),
            status: AgentRunStatus::Running,
        }
    }

    #[test]
    fn fixture_loads_and_names_all_units() {
        let fx = fixture();
        assert_eq!(fx["fixture"], Value::String("F18-09".to_owned()));
        for unit in [
            "user_has_issue_permission",
            "run_belongs_to",
            "resolve_moved_by_run",
            "active_run_of_caller",
            "refuse_agent_action",
            "permission_matrix",
            "page_archive_guard",
        ] {
            assert!(fx["units"][unit]["matrix"].is_object(), "{unit} matrix");
        }
    }

    #[test]
    fn class_denial_body_is_byte_identical_drf_default() {
        // DRF `PermissionDenied.default_detail`, compact separators.
        assert_eq!(
            CLASS_DENIAL_BODY,
            r#"{"detail":"You do not have permission to perform this action."}"#
        );
        assert_eq!(CLASS_DENIAL_BODY, crate::permissions::DEFAULT_DENIED_BODY);
        assert_eq!(
            UNAUTHENTICATED_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
    }

    #[test]
    fn gate_table_pins_every_route_and_method() {
        use V1WorkItemsGate as G;
        use V1WorkItemsRoute as R;
        // (route, registered methods, gate): methods from `http_method_names`
        // in `api/urls/{work_item,label,page}.py`.
        let cases: &[(R, &[&str], G)] = &[
            (R::OldSearch, &["GET"], G::AuthOnly),
            (R::OldByIdentifier, &["GET"], G::ProjectEntity),
            (R::OldIssueList, &["GET", "POST"], G::ProjectEntity),
            (
                R::OldIssueDetail,
                &["GET", "PATCH", "DELETE"],
                G::ProjectEntity,
            ),
            (R::OldLinkList, &["GET", "POST"], G::ProjectEntity),
            (
                R::OldLinkDetail,
                &["GET", "PATCH", "DELETE"],
                G::ProjectEntity,
            ),
            (R::OldCommentList, &["GET", "POST"], G::ProjectLite),
            (
                R::OldCommentDetail,
                &["GET", "PATCH", "DELETE"],
                G::ProjectLite,
            ),
            (R::OldActivityList, &["GET"], G::ProjectEntity),
            (R::OldActivityDetail, &["GET"], G::ProjectEntity),
            (R::OldAttachmentList, &["GET", "POST"], G::AuthOnly),
            (
                R::OldAttachmentDetail,
                &["GET", "PATCH", "DELETE"],
                G::AuthOnly,
            ),
            (R::Search, &["GET"], G::AuthOnly),
            (R::SearchAdvanced, &["GET"], G::AuthOnly),
            (R::ByIdentifier, &["GET"], G::ProjectEntity),
            (R::IssueList, &["GET", "POST"], G::ProjectEntity),
            (
                R::IssueDetail,
                &["GET", "PATCH", "DELETE"],
                G::ProjectEntity,
            ),
            (R::IssueMove, &["POST"], G::ProjectEntity),
            (R::IssueReTick, &["POST"], G::ProjectEntity),
            (R::IssueWait, &["POST"], G::ProjectEntity),
            (R::IssueRunAi, &["POST"], G::ProjectEntity),
            (R::AgentRunYield, &["POST"], G::AuthOnly),
            (R::LinkList, &["GET", "POST"], G::ProjectEntity),
            (R::LinkDetail, &["GET", "PATCH", "DELETE"], G::ProjectEntity),
            (R::CommentList, &["GET", "POST"], G::ProjectLite),
            (
                R::CommentDetail,
                &["GET", "PATCH", "DELETE"],
                G::ProjectLite,
            ),
            (R::ActivityList, &["GET"], G::ProjectEntity),
            (R::ActivityDetail, &["GET"], G::ProjectEntity),
            (R::AttachmentList, &["GET", "POST"], G::AuthOnly),
            (
                R::AttachmentDetail,
                &["GET", "PATCH", "DELETE"],
                G::AuthOnly,
            ),
            (R::RelationList, &["GET", "POST"], G::ProjectEntity),
            (R::RelationGrouped, &["GET"], G::ProjectEntity),
            (R::RelationRelate, &["POST"], G::ProjectEntity),
            (R::RelationUnrelate, &["POST"], G::ProjectEntity),
            (R::Workpad, &["GET", "PATCH"], G::ProjectEntity),
            (R::GithubPrList, &["GET", "POST"], G::ProjectEntity),
            (R::GithubPrDetail, &["DELETE"], G::ProjectEntity),
            (R::CodeReviewList, &["GET", "POST"], G::ProjectEntity),
            (R::CodeReviewDetail, &["DELETE"], G::ProjectEntity),
            (R::LabelList, &["GET", "POST"], G::ProjectMember),
            (
                R::LabelDetail,
                &["GET", "PATCH", "DELETE"],
                G::ProjectMember,
            ),
            (R::PageList, &["GET", "POST"], G::ProjectEntity),
            (R::PageDetail, &["GET", "PATCH"], G::ProjectEntity),
            (R::PageArchive, &["POST", "DELETE"], G::ProjectEntity),
        ];
        // 12 deprecated twins + 27 work-items + 2 label + 3 page.
        assert_eq!(cases.len(), 44);
        for (route, methods, gate) in cases {
            for method in *methods {
                assert_eq!(gate_for(*route, method), *gate, "{route:?} {method}");
            }
            // No class overrides `get_permissions`: unlisted methods map
            // the same gate (method dispatch / 405 is the router's job).
            for method in ["GET", "POST", "PATCH", "DELETE", "HEAD", "PUT"] {
                assert_eq!(gate_for(*route, method), *gate, "{route:?} {method}");
            }
        }
    }

    #[test]
    fn user_has_issue_permission_replay() {
        let m = matrix(&fixture(), "user_has_issue_permission");
        let creator = user();
        let other = stranger();
        // (issue_created_by, membership_exists, allow_creator)
        let cases: &[(&str, Option<Uuid>, bool, bool)] = &[
            ("creator_allow", Some(creator), false, true),
            ("creator_deny_flag", Some(creator), true, false),
            ("member_admin", Some(other), true, true),
            ("guest_in_roles", Some(other), true, true),
            ("guest_not_in_roles", Some(other), false, true),
            ("guest_no_roles_filter", Some(other), true, true),
            ("outsider", Some(other), false, true),
            ("issue_none_admin", None, true, true),
            ("issue_none_outsider", None, false, true),
        ];
        assert_eq!(m.as_object().unwrap().len(), cases.len());
        for (case, created_by, membership, allow_creator) in cases {
            assert_eq!(
                user_has_issue_permission(creator, *created_by, *membership, *allow_creator),
                m[*case].as_bool().unwrap(),
                "{case}",
            );
        }
    }

    #[test]
    fn run_belongs_to_replay() {
        let m = matrix(&fixture(), "run_belongs_to");
        assert_eq!(m.as_object().unwrap().len(), 6);
        let me = user();
        let other = stranger();
        let base = own_run("32bbc5d2-1407-45d8-8df2-266935de26e7", other, issue());
        // (user, run override): each case isolates one ownership path.
        let created_by = RunFacts {
            created_by_id: me,
            ..base
        };
        let owner_field = RunFacts {
            owner_id: Some(me),
            ..base
        };
        let runner_owner = RunFacts {
            runner_id: Some(Uuid::parse_str("55555555-5555-5555-5555-555555555555").unwrap()),
            runner_owner_id: Some(me),
            ..base
        };
        assert_eq!(
            run_belongs_to(None, Some(&created_by)),
            m["none_user"].as_bool().unwrap()
        );
        assert_eq!(
            run_belongs_to(Some(me), None),
            m["none_run"].as_bool().unwrap()
        );
        assert_eq!(
            run_belongs_to(Some(me), Some(&created_by)),
            m["created_by"].as_bool().unwrap()
        );
        assert_eq!(
            run_belongs_to(Some(me), Some(&owner_field)),
            m["owner_field"].as_bool().unwrap()
        );
        assert_eq!(
            run_belongs_to(Some(me), Some(&runner_owner)),
            m["runner_owner"].as_bool().unwrap()
        );
        assert_eq!(
            run_belongs_to(Some(me), Some(&base)),
            m["stranger"].as_bool().unwrap()
        );
        // A runner row without an id never grants ownership, even with a
        // stale owner id beside it (the `runner_id is not None` conjunct).
        let dangling = RunFacts {
            runner_id: None,
            runner_owner_id: Some(me),
            ..base
        };
        assert!(!run_belongs_to(Some(me), Some(&dangling)));
    }

    #[test]
    fn resolve_moved_by_run_replay() {
        let m = matrix(&fixture(), "resolve_moved_by_run");
        assert_eq!(m.as_object().unwrap().len(), 9);
        let me = user();
        let own_id = "32bbc5d2-1407-45d8-8df2-266935de26e7";
        let own = own_run(own_id, me, issue());
        let expect = |case: &str,
                      header: Option<&str>,
                      run_by_id: Option<&RunFacts>,
                      newest: &[RunFacts]| {
            let (run, error) = resolve_moved_by_run(header, Some(me), issue(), run_by_id, newest);
            let want_run = m[case]["run"].as_str().map(str::to_owned);
            assert_eq!(run.map(|id| id.to_string()), want_run, "{case} run");
            let want_error = m[case]["error"].as_str().map(str::to_owned);
            assert_eq!(
                error.map(|e| e.message().to_owned()),
                want_error,
                "{case} error"
            );
        };
        expect("bad_uuid", Some("not-a-uuid"), None, &[]);
        expect(
            "unknown_uuid",
            Some("66666666-6666-6666-6666-666666666666"),
            None,
            &[],
        );
        let other_issue_run = own_run("77777777-7777-7777-7777-777777777777", me, other_issue());
        expect(
            "other_issue_run",
            Some("77777777-7777-7777-7777-777777777777"),
            Some(&other_issue_run),
            &[],
        );
        let finished = RunFacts {
            status: AgentRunStatus::Completed,
            ..own
        };
        expect("finished_run", Some(own_id), Some(&finished), &[]);
        expect("no_header_owner_infers", None, None, &[own]);
        assert!(own.status.is_active());
        expect("own_active_run", Some(own_id), Some(&own), &[]);
        let foreign = own_run("88888888-8888-8888-8888-888888888888", stranger(), issue());
        expect(
            "foreign_run",
            Some("88888888-8888-8888-8888-888888888888"),
            Some(&foreign),
            &[],
        );
        // An outsider infers their own run: ownership, not membership.
        let outsider_run = own_run("faeeabd9-b92e-4e9b-bcb6-9039c786c6d5", me, issue());
        let (run, error) = resolve_moved_by_run(None, Some(me), issue(), None, &[outsider_run]);
        assert_eq!(
            run.map(|id| id.to_string()),
            m["no_header_outsider_infers_own"]["run"]
                .as_str()
                .map(str::to_owned)
        );
        assert_eq!(error, None);
        // A guest infers a run owned through the runner join.
        let guest_run = RunFacts {
            id: Uuid::parse_str("a0dd8187-1b0b-4e22-a19c-118229c3bbc8").unwrap(),
            created_by_id: stranger(),
            runner_id: Some(Uuid::parse_str("99999999-9999-9999-9999-999999999999").unwrap()),
            runner_owner_id: Some(me),
            work_item_id: Some(issue()),
            status: AgentRunStatus::Running,
            ..own
        };
        let (run, error) = resolve_moved_by_run(None, Some(me), issue(), None, &[guest_run]);
        assert_eq!(
            run.map(|id| id.to_string()),
            m["no_header_guest_infers_runner_run"]["run"]
                .as_str()
                .map(str::to_owned)
        );
        assert_eq!(error, None);
    }

    #[test]
    fn active_run_of_caller_replay() {
        let m = matrix(&fixture(), "active_run_of_caller");
        assert_eq!(m.as_object().unwrap().len(), 3);
        let me = user();
        let owner_run = own_run("32bbc5d2-1407-45d8-8df2-266935de26e7", me, issue());
        let outsider_run = own_run("faeeabd9-b92e-4e9b-bcb6-9039c786c6d5", me, issue());
        let guest_run = RunFacts {
            id: Uuid::parse_str("a0dd8187-1b0b-4e22-a19c-118229c3bbc8").unwrap(),
            created_by_id: stranger(),
            runner_id: Some(Uuid::parse_str("99999999-9999-9999-9999-999999999999").unwrap()),
            runner_owner_id: Some(me),
            work_item_id: Some(issue()),
            status: AgentRunStatus::Assigned,
            ..owner_run
        };
        for (case, run) in [
            ("owner", owner_run),
            ("outsider_sees_own", outsider_run),
            ("guest_runner_owned_run", guest_run),
        ] {
            // Newest-first input with noise a real fetch returns: a
            // finished own run and a foreign active run sort before the
            // match without being picked.
            let finished = RunFacts {
                status: AgentRunStatus::Failed,
                ..run
            };
            let foreign = RunFacts {
                id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
                created_by_id: stranger(),
                owner_id: None,
                runner_id: None,
                runner_owner_id: None,
                ..run
            };
            let found = active_run_of_caller(Some(me), issue(), &[finished, foreign, run]);
            assert_eq!(
                found.map(|id| id.to_string()),
                m[case].as_str().map(str::to_owned),
                "{case}"
            );
        }
        // The Python `[:5]` slice: a match past the fifth row is invisible.
        let filler = RunFacts {
            status: AgentRunStatus::Failed,
            ..owner_run
        };
        let beyond = [filler, filler, filler, filler, filler, owner_run];
        assert_eq!(active_run_of_caller(Some(me), issue(), &beyond), None);
        // Another issue's rows never resolve, even newest.
        let elsewhere = RunFacts {
            work_item_id: Some(other_issue()),
            ..owner_run
        };
        assert_eq!(active_run_of_caller(Some(me), issue(), &[elsewhere]), None);
        assert_eq!(active_run_of_caller(None, issue(), &[owner_run]), None);
    }

    #[test]
    fn refuse_agent_action_replay() {
        let m = matrix(&fixture(), "refuse_agent_action");
        assert_eq!(m.as_object().unwrap().len(), 7);
        let me = user();
        let own_id = "32bbc5d2-1407-45d8-8df2-266935de26e7";
        let own = own_run(own_id, me, issue());
        assert_eq!(
            refuse_agent_action(None, issue(), Some(&own), "Run AI"),
            None
        );
        assert_eq!(
            refuse_agent_action(Some("  "), issue(), Some(&own), "Run AI"),
            None,
            "blank header is no header"
        );
        assert_eq!(
            refuse_agent_action(Some("not-a-uuid"), issue(), None, "Run AI"),
            None,
            "bad_uuid_allowed"
        );
        assert!(m["bad_uuid_allowed"].is_null());
        let elsewhere = RunFacts {
            work_item_id: Some(other_issue()),
            ..own
        };
        assert_eq!(
            refuse_agent_action(Some(own_id), issue(), Some(&elsewhere), "Run AI"),
            None,
            "other_issue_allowed"
        );
        assert!(m["other_issue_allowed"].is_null());
        let finished = RunFacts {
            status: AgentRunStatus::Completed,
            ..own
        };
        assert_eq!(
            refuse_agent_action(Some(own_id), issue(), Some(&finished), "Run AI"),
            None,
            "finished_allowed"
        );
        assert!(m["finished_allowed"].is_null());
        assert!(m["no_header"].is_null());
        let refused = refuse_agent_action(Some(own_id), issue(), Some(&own), "Run AI")
            .expect("own_active_runai refuses");
        assert_eq!(
            refused.status,
            m["own_active_runai"]["status"].as_u64().unwrap() as u16
        );
        body_is(&refused.body, &m["own_active_runai"]["body"]);
        let retick = refuse_agent_retick(Some(own_id), issue(), Some(&own))
            .expect("own_active_retick refuses");
        assert_eq!(
            retick.status,
            m["own_active_retick"]["status"].as_u64().unwrap() as u16
        );
        body_is(&retick.body, &m["own_active_retick"]["body"]);
        // BUG PORTED: ownership is never checked — a foreign active run on
        // this issue refuses exactly like an owned one.
        let foreign = RunFacts {
            created_by_id: stranger(),
            ..own
        };
        let refused_foreign = refuse_agent_action(Some(own_id), issue(), Some(&foreign), "Run AI")
            .expect("foreign_active_on_issue_refused refuses");
        assert_eq!(
            refused_foreign.status,
            m["foreign_active_on_issue_refused"]["status"]
                .as_u64()
                .unwrap() as u16
        );
        body_is(
            &refused_foreign.body,
            &m["foreign_active_on_issue_refused"]["body"],
        );
    }

    #[test]
    fn uuid_spellings_match_python_uuid() {
        // `uuid.UUID` accepts all four spellings; so must the header parse.
        let hyphenated = "32bbc5d2-1407-45d8-8df2-266935de26e7";
        let spellings = [
            hyphenated.to_owned(),
            "32bbc5d2140745d88df2266935de26e7".to_owned(),
            "{32bbc5d2-1407-45d8-8df2-266935de26e7}".to_owned(),
            "urn:uuid:32bbc5d2-1407-45d8-8df2-266935de26e7".to_owned(),
            "  32bbc5d2-1407-45d8-8df2-266935de26e7  ".to_owned(),
        ];
        let me = user();
        let own = own_run(hyphenated, me, issue());
        for header in &spellings {
            let (run, error) =
                resolve_moved_by_run(Some(header), Some(me), issue(), Some(&own), &[]);
            assert_eq!((run, error), (Some(own.id), None), "{header}");
        }
        // …and both reject the same malformed inputs.
        for header in [
            "",
            "   ",
            "not-a-uuid",
            "32bbc5d2-1407",
            "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
        ] {
            let (_, error) = resolve_moved_by_run(Some(header), Some(me), issue(), Some(&own), &[]);
            if header.trim().is_empty() {
                assert_eq!(error, None, "{header:?} infers");
            } else {
                assert_eq!(error, Some(ResolveRunError::NotUuid), "{header:?}");
            }
            assert_eq!(
                refuse_agent_action(Some(header), issue(), Some(&own), "Run AI"),
                None,
                "{header:?} is allowed through the refusal"
            );
        }
    }

    #[test]
    fn resolve_error_bodies_are_byte_exact() {
        assert_eq!(
            ResolveRunError::NotUuid.message(),
            "X-Pi-Dash-Run-Id is not a UUID"
        );
        assert_eq!(
            ResolveRunError::NotYours.message(),
            "X-Pi-Dash-Run-Id names a run that is not yours"
        );
        assert_eq!(
            HEADER_NOT_UUID_BODY,
            r#"{"error":"X-Pi-Dash-Run-Id is not a UUID"}"#
        );
        assert_eq!(
            HEADER_NOT_YOURS_BODY,
            r#"{"error":"X-Pi-Dash-Run-Id names a run that is not yours"}"#
        );
        assert_eq!(ResolveRunError::NotUuid.body(), HEADER_NOT_UUID_BODY);
        assert_eq!(ResolveRunError::NotYours.body(), HEADER_NOT_YOURS_BODY);
        // The 400 bodies parse back to the messages (move PATCH `:804`).
        for error in [ResolveRunError::NotUuid, ResolveRunError::NotYours] {
            let parsed: Value = serde_json::from_str(error.body()).unwrap();
            assert_eq!(parsed, serde_json::json!({"error": error.message()}));
        }
    }

    #[test]
    fn permission_matrix_replay() {
        use V1WorkItemsGate as G;
        let m = matrix(&fixture(), "permission_matrix");
        assert_eq!(m.as_object().unwrap().len(), 11);
        // (case, gate, method, facts, allow?): facts built per fixture principal.
        let outsider = project_facts();
        let mut guest = project_facts();
        project_guest(&mut guest);
        let mut member = project_facts();
        project_member(&mut member);
        // A label-POST guest: project guest without a workspace ADMIN/MEMBER row.
        let mut label_guest = project_facts();
        label_guest.is_workspace_member = true;
        label_guest.is_project_member = true;
        // By-identifier SAFE reads the identifier's project, not view.project_id.
        let mut identifier_member = project_facts();
        identifier_member.has_project_identifier = true;
        identifier_member.has_identifier_membership = true;
        let cases: &[(&str, G, &str, &project::ProjectFacts, bool)] = &[
            ("guest_list_safe", G::ProjectEntity, "GET", &guest, true),
            ("outsider_list", G::ProjectEntity, "GET", &outsider, false),
            (
                "guest_create_entity",
                G::ProjectEntity,
                "POST",
                &guest,
                false,
            ),
            (
                "member_create_entity",
                G::ProjectEntity,
                "POST",
                &member,
                true,
            ),
            (
                "guest_comment_create_lite",
                G::ProjectLite,
                "POST",
                &guest,
                true,
            ),
            (
                "guest_label_create_memberperm",
                G::ProjectMember,
                "POST",
                &label_guest,
                false,
            ),
            ("guest_label_list", G::ProjectMember, "GET", &guest, true),
            (
                "outsider_label_list",
                G::ProjectMember,
                "GET",
                &outsider,
                false,
            ),
            (
                "inactive_member_list",
                G::ProjectEntity,
                "GET",
                &outsider,
                false,
            ),
            (
                "archived_project_list",
                G::ProjectEntity,
                "GET",
                &member,
                true,
            ),
        ];
        for (case, gate, method, facts, allow) in cases {
            assert_eq!(
                decide(*gate, method, &scope(), facts),
                *allow,
                "{case} decision"
            );
        }
        // Statuses: allow → the recorded 2xx, deny → 403 (401 for anonymous).
        let mut anonymous = project_facts();
        anonymous.authenticated = false;
        assert!(!decide(G::ProjectEntity, "GET", &scope(), &anonymous));
        assert_eq!(m["anon_list"]["status"], Value::from(401));
        body_is(UNAUTHENTICATED_BODY, &m["anon_list"]["body"]);
        for (case, allow) in [
            ("guest_list_safe", true),
            ("outsider_list", false),
            ("guest_create_entity", false),
            ("member_create_entity", true),
            ("guest_comment_create_lite", true),
            ("guest_label_create_memberperm", false),
            ("guest_label_list", true),
            ("outsider_label_list", false),
            ("inactive_member_list", false),
            ("archived_project_list", true),
        ] {
            let status = m[case]["status"].as_u64().unwrap();
            if allow {
                assert!(status == 200 || status == 201, "{case} status {status}");
            } else {
                assert_eq!(status, 403, "{case} status");
                body_is(CLASS_DENIAL_BODY, &m[case]["body"]);
            }
        }
        // 200/201 bodies are handler payloads, not guard output — the guard
        // replay pins allow + status for those (shapes live in F18-01..04).
        assert_eq!(m["guest_list_safe"]["status"], Value::from(200));
        assert_eq!(m["member_create_entity"]["status"], Value::from(201));
        assert_eq!(m["guest_comment_create_lite"]["status"], Value::from(201));
        assert_eq!(m["guest_label_list"]["status"], Value::from(200));
        assert_eq!(m["archived_project_list"]["status"], Value::from(200));
        // The by-identifier Entity SAFE branch reads the identifier fact.
        assert!(decide(
            G::ProjectEntity,
            "GET",
            &scope(),
            &identifier_member
        ));
        let mut no_identifier = project_facts();
        no_identifier.has_project_identifier = true;
        assert!(!decide(G::ProjectEntity, "GET", &scope(), &no_identifier));
    }

    #[test]
    fn delete_guard_matrix() {
        // No fixture cases (pinned from `views/issue.py:885-897`): the
        // creator or a project admin may delete; anyone else 403s.
        let me = user();
        assert!(can_delete_work_item(me, me, false));
        assert!(can_delete_work_item(me, stranger(), true));
        assert!(!can_delete_work_item(me, stranger(), false));
        assert!(can_delete_work_item(me, me, true));
        assert_eq!(
            DELETE_DENIAL_BODY,
            r#"{"error":"Only admin or creator can delete the work item"}"#
        );
    }

    #[test]
    fn page_archive_guard_replay() {
        use V1WorkItemsGate as G;
        let m = matrix(&fixture(), "page_archive_guard");
        assert_eq!(m.as_object().unwrap().len(), 6);
        let owner = user();
        let member = stranger();
        // guest_archive: Entity POST refuses the guest before the guard runs.
        let mut guest = project_facts();
        project_guest(&mut guest);
        assert!(!decide(G::ProjectEntity, "POST", &scope(), &guest));
        assert_eq!(m["guest_archive"]["status"], Value::from(403));
        body_is(CLASS_DENIAL_BODY, &m["guest_archive"]["body"]);
        // Owner paths allow (archive / noop / unarchive are handler 200s).
        for case in [
            "owner_archive",
            "owner_archive_again_noop",
            "owner_unarchive",
        ] {
            assert_eq!(
                check_can_archive(false, owner, owner, false),
                ArchiveDecision::Allow,
                "{case}"
            );
            assert_eq!(m[case]["status"], Value::from(200), "{case} status");
        }
        // A project admin who is not the owner passes too (source `:497`).
        assert_eq!(
            check_can_archive(false, owner, member, true),
            ArchiveDecision::Allow
        );
        // Lock first: even the owner 409s, with the exact `_error` bytes.
        assert_eq!(
            check_can_archive(true, owner, owner, true),
            ArchiveDecision::Locked
        );
        assert_eq!(m["locked_archive"]["status"], Value::from(409));
        body_is(PAGE_LOCKED_BODY, &m["locked_archive"]["body"]);
        assert_eq!(PAGE_LOCKED_CODE, 4701);
        assert_eq!(
            PAGE_LOCKED_BODY,
            r#"{"error":"Page is locked","error_code":4701,"error_message":"PAGE_LOCKED"}"#
        );
        // A project MEMBER who is not the owner 403s with the owner/admin message.
        assert_eq!(
            check_can_archive(false, owner, member, false),
            ArchiveDecision::Forbidden
        );
        assert_eq!(m["member_nonowner_archive"]["status"], Value::from(403));
        body_is(
            PAGE_OWNER_DENIAL_BODY,
            &m["member_nonowner_archive"]["body"],
        );
        assert_eq!(ArchiveDecision::Locked.status(), Some(409));
        assert_eq!(ArchiveDecision::Forbidden.status(), Some(403));
        assert_eq!(ArchiveDecision::Allow.status(), None);
        assert_eq!(ArchiveDecision::Allow.body(), None);
    }

    #[test]
    fn attachment_denial_bodies_are_byte_exact() {
        assert_eq!(
            ATTACHMENT_UPLOAD_DENIAL_BODY,
            r#"{"error":"You are not allowed to upload this attachment"}"#
        );
        assert_eq!(
            ATTACHMENT_DELETE_DENIAL_BODY,
            r#"{"error":"You are not allowed to delete this attachment"}"#
        );
        assert_eq!(
            ATTACHMENT_DOWNLOAD_DENIAL_BODY,
            r#"{"error":"You are not allowed to download this attachment"}"#
        );
    }
}
