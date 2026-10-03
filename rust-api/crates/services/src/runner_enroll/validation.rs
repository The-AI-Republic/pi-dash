#![forbid(unsafe_code)]

//! Run-creation validation (services-B, PIDASHCONV-587).
//!
//! Port of `validate_run_creation` + `_resolve_pod`
//! (`runner/services/validation.py:28-182`): the §6.5 pre-create checks
//! shared by the direct run-creation endpoint and the orchestration path.
//! The only production caller is D-15 (`runner/views/runs.py:247`); D-13
//! ports it unblocked and D-15 consumes it through [`RunCreationStore`].
//!
//! # Shape: store seam, not inline SQL
//!
//! The services crate has no `sqlx` dependency, so — following the
//! `integrations/accounts.rs` `GitStore` precedent — the reads live behind
//! [`RunCreationStore`]: one async method per Django ORM call, with the
//! SQL text in the adjacent `*_SQL` consts, which the pool implementation
//! must execute verbatim. [`validate_run_creation`] keeps the full
//! orchestration (including the conditional-read order), so the D-15
//! handler issues exactly the reads Python issues, in the same order.
//!
//! Membership goes through the kernel
//! ([`pidash_auth::permissions::membership::is_workspace_member`]), never
//! re-ported: the store fetches the role ([`WORKSPACE_ROLE_SQL`], the
//! `workspace_role` read the kernel's contract names) and the kernel
//! decides. Python's `is_workspace_member` uses `.exists()` where the port
//! fetches the role — observationally identical (both are existence
//! checks), and the kernel's `Option<i32>` input requires the fetch.
//!
//! # SQL provenance
//!
//! Every `*_SQL` const is byte-identical to `str(qs.query)` compiled from
//! the real querysets on the repo-pinned Django 4.2.30 (`%s` rendered as
//! `$N`; `.first()` executes with `LIMIT 1`, which the recorded query
//! shows once sliced). The `#[cfg(test)]` suite pins the full text.
//!
//! # Typed boundary
//!
//! Python takes the raw request values (`request.data.get("workspace")`,
//! ...), so a garbage UUID string reaches the ORM and Django raises
//! `ValidationError` (a 500) inside the membership filter. Request parsing
//! stays the D-15 handler's job here: this module takes typed
//! `Option<Uuid>` inputs (`None` covers missing *and* empty — the falsy
//! arm), and unparseable values are the handler's 400. Likewise `str()`
//! UUID comparisons become direct `Uuid ==` (both sides canonical).
//!
//! # Fixture source of truth
//!
//! `rust-api/fixtures/runner_enroll/services/flows.golden.json`
//! (`validate_run_creation`: the 8 status/code branches, the `issue_probe`
//! ORM-call pin, the `pod_priority` order, the no-writes `returns` note).
//! Each `#[cfg(test)]` suite replays its section. D13-F5 carries no
//! dedicated run-validation section (its pod/project rows are
//! hand-composed `SELECT *` abbreviations), so the F5 direction is met by
//! the Django-exact consts plus fragment pins.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * No bugs found in this unit on read-through. Sharp edges ported as-is:
//! * QUIRK-pinned-leniency (`validation.py:148-156`): the issue-pinned pod
//!   is lenient — soft-deleted, wrong-workspace, or wrong-project falls
//!   through silently to the default legs — while the explicit pod
//!   (`:130-144`) hard-fails each of those with its own 400. The
//!   asymmetry is deliberate (commented at `:156`) and preserved.
//! * QUIRK-workspace-gate-dead (`:166`): `if workspace_id is not None` is
//!   always true past the falsy check at `:67`. Ported by construction:
//!   the type system proves it (non-optional `Uuid` past the gate).
//! * DIVERGENCE-default-where-order: real Django emits the
//!   `default_for_project_id` scope as `(deleted_at IS NULL AND is_default
//!   AND project_id = $1)` (`Q` sorts kwargs), matching the D-15
//!   `runner_runs::pod::DEFAULT_FOR_PROJECT_ID_SQL` const. The D-13 models
//!   `runner_enroll::columns::pod::DEFAULT_FOR_PROJECT_ID_SQL` const lists
//!   `project_id` first. `AND` commutes, so both return identical rows;
//!   [`DEFAULT_POD_FOR_PROJECT_SQL`] keeps the verified Django order and
//!   `columns.rs` is outside this issue's paths, so that const is left
//!   for its owner.

use pidash_auth::permissions::membership;
use pidash_db::runner_runs::pod::Pod;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Read SQL (one const per ORM call; `$N` in binding order)
// ---------------------------------------------------------------------------

/// `workspace_role` read (`core/permissions.py:37-45`) feeding the
/// membership kernel: newest active row's role. `$1` is the user id, `$2`
/// the workspace id. Same text as the D-12 `WORKSPACE_ROLE_SQL` twin —
/// copied (not imported) per this issue's own-SQL sanction, so no
/// cross-domain code dependency enters.
pub const WORKSPACE_ROLE_SQL: &str = "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = $1 AND \"workspace_members\".\"workspace_id\" = $2) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1";

/// Work-item consistency probe (`validation.py:80-84`):
/// `Issue.objects.filter(pk).values("workspace_id", "assigned_pod_id",
/// "project_id").first()`. The default `Issue.objects` manager is the
/// plain soft-delete scope (`deleted_at IS NULL` only — the triage /
/// archived exclusions live on `issue_objects`, not `objects`). `$1` is
/// the work-item id.
pub const ISSUE_PROBE_SQL: &str = "SELECT \"issues\".\"workspace_id\", \"issues\".\"assigned_pod_id\", \"issues\".\"project_id\" FROM \"issues\" WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"id\" = $1) ORDER BY \"issues\".\"created_at\" DESC LIMIT 1";

/// Pod lookup by id (`validation.py:131,149`):
/// `Pod.objects.filter(pk).first()`, shared by the explicit and pinned
/// legs. The `PodManager` scope makes soft-deleted pods read as missing.
/// `$1` is the pod id.
pub const POD_BY_ID_SQL: &str = "SELECT \"pod\".\"id\", \"pod\".\"workspace_id\", \"pod\".\"project_id\", \"pod\".\"name\", \"pod\".\"description\", \"pod\".\"created_by_id\", \"pod\".\"is_default\", \"pod\".\"deleted_at\", \"pod\".\"created_at\", \"pod\".\"updated_at\" FROM \"pod\" WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"id\" = $1) ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC LIMIT 1";

/// Single-project back-compat probe (`validation.py:169-172`):
/// `Project.objects.filter(workspace_id).values_list("id")[:2]`. The
/// soft-delete scope applies; `-created_at` ordering comes from
/// `Meta.ordering`. `$1` is the workspace id.
pub const PROJECT_IDS_FOR_WORKSPACE_SQL: &str = "SELECT \"projects\".\"id\" FROM \"projects\" WHERE (\"projects\".\"deleted_at\" IS NULL AND \"projects\".\"workspace_id\" = $1) ORDER BY \"projects\".\"created_at\" DESC LIMIT 2";

/// `Pod.default_for_project_id` (`runner/models.py:174-176`):
/// `objects.filter(project_id, is_default=True).first()`. Conjunct order
/// is Django's (`Q`-sorted kwargs after the manager scope), verified
/// against the real queryset — see DIVERGENCE-default-where-order above.
/// `$1` is the project id.
pub const DEFAULT_POD_FOR_PROJECT_SQL: &str = "SELECT \"pod\".\"id\", \"pod\".\"workspace_id\", \"pod\".\"project_id\", \"pod\".\"name\", \"pod\".\"description\", \"pod\".\"created_by_id\", \"pod\".\"is_default\", \"pod\".\"deleted_at\", \"pod\".\"created_at\", \"pod\".\"updated_at\" FROM \"pod\" WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"is_default\" AND \"pod\".\"project_id\" = $1) ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC LIMIT 1";

// ---------------------------------------------------------------------------
// Row facts + store seam
// ---------------------------------------------------------------------------

/// The work-item consistency probe row ([`ISSUE_PROBE_SQL`]).
///
/// `project_id` is `Option` for the defensive `(issue or {}).get(...)`
/// reads (`validation.py:100,105`); the column is `NOT NULL`, so a live
/// row always carries `Some`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IssueProbe {
    /// `issue.workspace_id` (`:87` comparison).
    pub workspace_id: Uuid,
    /// `issue.assigned_pod_id` (the lenient pinned leg, `:148`).
    pub assigned_pod_id: Option<Uuid>,
    /// `issue.project_id` (the project anchor, `:100`).
    pub project_id: Option<Uuid>,
}

/// Storage failure for the [`RunCreationStore`] reads.
///
/// All five reads are `Option`/`Vec` shaped, so only the database-error
/// arm exists (no `NotFound`: a miss is `None`, mirroring `.first()`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// Any database failure; the handler renders it a 500 (Python lets
    /// the DB exception propagate out of `validate_run_creation`).
    #[error("database error: {0}")]
    Db(String),
}

/// Storage seam for run-creation validation.
///
/// Methods mirror the Django ORM calls in `validation.py`, one per query
/// shape; the SQL text for each lives in the adjacent `*_SQL` consts,
/// which the pool implementation must execute verbatim. Reads only — the
/// unit performs no writes (fixture `returns`).
///
/// Native `async fn` in trait (stable since 1.75): no `async-trait`
/// dependency enters the lockfile for this seam (the `GitStore`
/// precedent).
#[allow(async_fn_in_trait)]
pub trait RunCreationStore {
    /// Newest active `WorkspaceMember` role ([`WORKSPACE_ROLE_SQL`]);
    /// `None` when no live row exists.
    async fn workspace_role(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<Option<i32>, StoreError>;

    /// Work-item probe row ([`ISSUE_PROBE_SQL`]).
    async fn issue_probe(&self, work_item_id: Uuid) -> Result<Option<IssueProbe>, StoreError>;

    /// Live pod by id ([`POD_BY_ID_SQL`]); soft-deleted reads as `None`.
    async fn pod_by_id(&self, pod_id: Uuid) -> Result<Option<Pod>, StoreError>;

    /// Live default pod for a project ([`DEFAULT_POD_FOR_PROJECT_SQL`]).
    async fn default_pod_for_project(&self, project_id: Uuid) -> Result<Option<Pod>, StoreError>;

    /// Live project ids for a workspace, newest first, at most two
    /// ([`PROJECT_IDS_FOR_WORKSPACE_SQL`]).
    async fn project_ids_for_workspace(&self, workspace_id: Uuid) -> Result<Vec<Uuid>, StoreError>;
}

// ---------------------------------------------------------------------------
// Error + context
// ---------------------------------------------------------------------------

/// `RunCreationError` (`validation.py:28-35`): pre-create validation
/// failure carrying an HTTP-ish status.
///
/// One variant per `status`/`message`/`code` triplet, in source order.
/// `Display` renders the message (`str(exc) == message` in Python, via
/// `super().__init__(message)`); the D-15 caller renders
/// `{"error": message, "code": code}` at `status` (`runs.py:253-257`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RunCreationError {
    /// `:68` — falsy `workspace_id`.
    #[error("workspace is required")]
    WorkspaceRequired,
    /// `:72-76` — caller is not a workspace member.
    #[error("caller is not a member of the target workspace")]
    NotWorkspaceMember,
    /// `:86` — `work_item_id` set but no live `Issue` row.
    #[error("work_item does not exist")]
    WorkItemMissing,
    /// `:88-92` — issue's workspace differs (string-compared in Python).
    #[error("work_item does not belong to workspace")]
    WorkItemWorkspaceMismatch,
    /// `:133` — explicit pod missing or soft-deleted.
    #[error("pod does not exist or has been deleted")]
    PodMissing,
    /// `:135-137` — explicit pod in another workspace.
    #[error("pod does not belong to workspace")]
    PodWorkspaceMismatch,
    /// `:139-143` — explicit pod in another project (project known).
    #[error("pod does not belong to issue's project")]
    PodProjectMismatch,
    /// `:178-182` — nothing resolved on any leg.
    #[error("no pod available; ensure the issue has a project with a default pod")]
    NoPodAvailable,
}

impl RunCreationError {
    /// The HTTP-ish status (`runs.py:256`).
    pub fn status(&self) -> u16 {
        match self {
            RunCreationError::WorkspaceRequired => 400,
            RunCreationError::NotWorkspaceMember => 403,
            RunCreationError::WorkItemMissing => 400,
            RunCreationError::WorkItemWorkspaceMismatch => 400,
            RunCreationError::PodMissing => 400,
            RunCreationError::PodWorkspaceMismatch => 400,
            RunCreationError::PodProjectMismatch => 400,
            RunCreationError::NoPodAvailable => 409,
        }
    }

    /// The machine code (`runs.py:255`).
    pub fn code(&self) -> &'static str {
        match self {
            RunCreationError::WorkspaceRequired => "workspace_required",
            RunCreationError::NotWorkspaceMember => "not_workspace_member",
            RunCreationError::WorkItemMissing => "work_item_missing",
            RunCreationError::WorkItemWorkspaceMismatch => "work_item_workspace_mismatch",
            RunCreationError::PodMissing => "pod_missing",
            RunCreationError::PodWorkspaceMismatch => "pod_workspace_mismatch",
            RunCreationError::PodProjectMismatch => "pod_project_mismatch",
            RunCreationError::NoPodAvailable => "no_pod_available",
        }
    }

    /// The human message — identical to `Display`.
    pub fn message(&self) -> &'static str {
        match self {
            RunCreationError::WorkspaceRequired => "workspace is required",
            RunCreationError::NotWorkspaceMember => {
                "caller is not a member of the target workspace"
            }
            RunCreationError::WorkItemMissing => "work_item does not exist",
            RunCreationError::WorkItemWorkspaceMismatch => "work_item does not belong to workspace",
            RunCreationError::PodMissing => "pod does not exist or has been deleted",
            RunCreationError::PodWorkspaceMismatch => "pod does not belong to workspace",
            RunCreationError::PodProjectMismatch => "pod does not belong to issue's project",
            RunCreationError::NoPodAvailable => {
                "no pod available; ensure the issue has a project with a default pod"
            }
        }
    }
}

/// `ValidatedRunContext` (`validation.py:38-45`): the verified inputs for
/// an `AgentRun` row about to be inserted.
///
/// `created_by` is the caller id Python passes through as the `User`
/// instance (`:112`, consumed as `actor` at `runs.py:271`).
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedRunContext {
    /// The verified workspace id (echoed, `:109`).
    pub workspace_id: Uuid,
    /// The verified work-item id, if one was given (echoed, `:110`).
    pub work_item_id: Option<Uuid>,
    /// The resolved pod (`:111`).
    pub pod: Pod,
    /// The calling user id (`:112`).
    pub created_by: Uuid,
}

/// Combined failure for [`validate_run_creation`]: a validation triplet
/// ([`RunCreationError`], the handler's 4xx) or a storage failure
/// ([`StoreError`], the handler's 500).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    /// A §6.5 check failed.
    #[error("{0}")]
    Invalid(#[from] RunCreationError),
    /// A store read failed.
    #[error("{0}")]
    Store(#[from] StoreError),
}

// ---------------------------------------------------------------------------
// Orchestration
// ---------------------------------------------------------------------------

/// Run the §6.5 checks and return a [`ValidatedRunContext`]
/// (`validation.py:48-113`).
///
/// Argument order mirrors Python (`user, workspace_id, *, work_item_id,
/// pod_id`); `None` is the missing/empty arm for every optional input
/// (see "Typed boundary" above). Reads issue in Python order —
/// membership, work item, then the pod legs — so the store sees exactly
/// the queries Python runs.
pub async fn validate_run_creation<S: RunCreationStore>(
    store: &S,
    user_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    work_item_id: Option<Uuid>,
    pod_id: Option<Uuid>,
) -> Result<ValidatedRunContext, ValidationError> {
    // `:67-68` — falsy workspace. First: Python checks before membership.
    let Some(workspace_id) = workspace_id else {
        return Err(RunCreationError::WorkspaceRequired.into());
    };

    // `:71-76` — workspace membership through the kernel. A `None` caller
    // short-circuits to the 403 with no query, exactly like the
    // `is_workspace_member` guard (`core/permissions.py:30-32`) and the
    // orchestration `fetch_*` convention.
    let Some(user_id) = user_id else {
        return Err(RunCreationError::NotWorkspaceMember.into());
    };
    let role = store.workspace_role(user_id, workspace_id).await?;
    if !membership::is_workspace_member(role) {
        return Err(RunCreationError::NotWorkspaceMember.into());
    }

    // `:79-94` — work-item consistency.
    let issue = match work_item_id {
        Some(work_item_id) => {
            let probe = store.issue_probe(work_item_id).await?;
            let probe = probe.ok_or(RunCreationError::WorkItemMissing)?;
            if probe.workspace_id != workspace_id {
                return Err(RunCreationError::WorkItemWorkspaceMismatch.into());
            }
            Some(probe)
        }
        None => None,
    };

    // `:100-106` — the project anchor and pinned pod flow into `_resolve_pod`.
    let project_id = issue.as_ref().and_then(|probe| probe.project_id);
    let issue_assigned_pod_id = issue.as_ref().and_then(|probe| probe.assigned_pod_id);
    let pod = resolve_pod(
        store,
        workspace_id,
        project_id,
        pod_id,
        issue_assigned_pod_id,
    )
    .await?;

    Ok(ValidatedRunContext {
        workspace_id,
        work_item_id,
        pod,
        created_by: user_id,
    })
}

/// Resolve the pod the run belongs to (`validation.py:116-182`).
///
/// Priority: explicit `pod_id` (strict) > issue-pinned pod (lenient:
/// mismatch/deleted falls through) > project default pod > single-project
/// back-compat. Private, like the Python original — D-15 consumes
/// [`validate_run_creation`] only.
async fn resolve_pod<S: RunCreationStore>(
    store: &S,
    workspace_id: Uuid,
    project_id: Option<Uuid>,
    pod_id: Option<Uuid>,
    issue_assigned_pod_id: Option<Uuid>,
) -> Result<Pod, ValidationError> {
    // `:130-144` — explicit pod: must exist, be live, and belong to the
    // workspace and (when known) the issue's project. Every violation is
    // its own hard 400.
    if let Some(pod_id) = pod_id {
        let pod = store
            .pod_by_id(pod_id)
            .await?
            .ok_or(RunCreationError::PodMissing)?;
        if pod.workspace_id != workspace_id {
            return Err(RunCreationError::PodWorkspaceMismatch.into());
        }
        if let Some(project_id) = project_id {
            if pod.project_id != project_id {
                return Err(RunCreationError::PodProjectMismatch.into());
            }
        }
        return Ok(pod);
    }

    // `:148-156` — the issue's pinned pod with the same checks, but
    // lenient: a soft-deleted or stale pin falls through silently.
    if let Some(pinned_id) = issue_assigned_pod_id {
        if let Some(pod) = store.pod_by_id(pinned_id).await? {
            let project_ok = project_id.is_none_or(|known| pod.project_id == known);
            if pod.workspace_id == workspace_id && project_ok {
                return Ok(pod);
            }
        }
        // Pinned pod was soft-deleted or stale — fall through.
    }

    // `:162-165` — the project's default pod.
    if let Some(project_id) = project_id {
        if let Some(default) = store.default_pod_for_project(project_id).await? {
            return Ok(default);
        }
    }

    // `:166-176` — back-compat: without a project, a workspace with
    // exactly one project uses that project's default. (The `:166`
    // `workspace_id is not None` gate is ported by construction — the
    // type proves it past the falsy check.)
    if let [only] = store
        .project_ids_for_workspace(workspace_id)
        .await?
        .as_slice()
    {
        if let Some(default) = store.default_pod_for_project(*only).await? {
            return Ok(default);
        }
    }

    // `:178-182` — nothing resolved on any leg.
    Err(RunCreationError::NoPodAvailable.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// The F6 golden file, loaded verbatim (sibling `include_str!`
    /// precedent: `runner_runs/guards.rs`).
    fn fixture() -> serde_json::Value {
        let text = include_str!("../../../../fixtures/runner_enroll/services/flows.golden.json");
        serde_json::from_str(text).expect("flows.golden.json parses")
    }

    fn fixture_branches() -> Vec<serde_json::Value> {
        fixture()["validate_run_creation"]["branches"]
            .as_array()
            .expect("branches is an array")
            .clone()
    }

    // ------------------------------------------------------------------
    // SQL: full verified text + F5 fragment pins
    // ------------------------------------------------------------------

    #[test]
    fn sql_matches_real_django_verbatim() {
        // Compiled from the real querysets on Django 4.2.30 (`%s` -> `$N`).
        assert_eq!(
            WORKSPACE_ROLE_SQL,
            "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = $1 AND \"workspace_members\".\"workspace_id\" = $2) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
        );
        assert_eq!(
            ISSUE_PROBE_SQL,
            "SELECT \"issues\".\"workspace_id\", \"issues\".\"assigned_pod_id\", \"issues\".\"project_id\" FROM \"issues\" WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"id\" = $1) ORDER BY \"issues\".\"created_at\" DESC LIMIT 1"
        );
        assert_eq!(
            POD_BY_ID_SQL,
            "SELECT \"pod\".\"id\", \"pod\".\"workspace_id\", \"pod\".\"project_id\", \"pod\".\"name\", \"pod\".\"description\", \"pod\".\"created_by_id\", \"pod\".\"is_default\", \"pod\".\"deleted_at\", \"pod\".\"created_at\", \"pod\".\"updated_at\" FROM \"pod\" WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"id\" = $1) ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC LIMIT 1"
        );
        assert_eq!(
            PROJECT_IDS_FOR_WORKSPACE_SQL,
            "SELECT \"projects\".\"id\" FROM \"projects\" WHERE (\"projects\".\"deleted_at\" IS NULL AND \"projects\".\"workspace_id\" = $1) ORDER BY \"projects\".\"created_at\" DESC LIMIT 2"
        );
        assert_eq!(
            DEFAULT_POD_FOR_PROJECT_SQL,
            "SELECT \"pod\".\"id\", \"pod\".\"workspace_id\", \"pod\".\"project_id\", \"pod\".\"name\", \"pod\".\"description\", \"pod\".\"created_by_id\", \"pod\".\"is_default\", \"pod\".\"deleted_at\", \"pod\".\"created_at\", \"pod\".\"updated_at\" FROM \"pod\" WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"is_default\" AND \"pod\".\"project_id\" = $1) ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC LIMIT 1"
        );
    }

    #[test]
    fn sql_pins_f5_fragments() {
        // The `assert_builder_contains` direction (D-02 `space/queries`
        // precedent): F5's hand-composed pod/project rows abbreviate the
        // Django renderings, so the recorded fragments must appear inside
        // the builder output, not equal it.
        for sql in [POD_BY_ID_SQL, DEFAULT_POD_FOR_PROJECT_SQL] {
            assert!(sql.contains("FROM \"pod\""), "{sql}");
            assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
            assert!(sql.contains("LIMIT 1"), "{sql}");
        }
        assert!(POD_BY_ID_SQL.contains("\"id\" = $1"));
        assert!(DEFAULT_POD_FOR_PROJECT_SQL.contains("\"is_default\""));
        assert!(DEFAULT_POD_FOR_PROJECT_SQL.contains("\"project_id\" = $1"));
        assert!(PROJECT_IDS_FOR_WORKSPACE_SQL.contains("FROM \"projects\""));
        assert!(PROJECT_IDS_FOR_WORKSPACE_SQL.contains("\"workspace_id\" = $1"));
        assert!(PROJECT_IDS_FOR_WORKSPACE_SQL.contains("LIMIT 2"));
        assert!(ISSUE_PROBE_SQL.contains("FROM \"issues\""));
        for column in ["\"workspace_id\"", "\"assigned_pod_id\"", "\"project_id\""] {
            assert!(ISSUE_PROBE_SQL.contains(column), "{column}");
        }
        // `.first()` keeps `Meta.ordering` on every leg.
        assert!(ISSUE_PROBE_SQL.contains("ORDER BY \"issues\".\"created_at\" DESC"));
        assert!(POD_BY_ID_SQL.contains("ORDER BY \"pod\".\"is_default\" DESC"));
        assert!(WORKSPACE_ROLE_SQL.contains("ORDER BY \"workspace_members\".\"created_at\" DESC"));
    }

    // ------------------------------------------------------------------
    // F6 replay: branches, probe pin, priority, returns
    // ------------------------------------------------------------------

    /// All eight triplets in source order.
    fn triplets() -> [(RunCreationError, u16, &'static str, &'static str); 8] {
        use RunCreationError::*;
        [
            (
                WorkspaceRequired,
                400,
                "workspace_required",
                "workspace is required",
            ),
            (
                NotWorkspaceMember,
                403,
                "not_workspace_member",
                "caller is not a member of the target workspace",
            ),
            (
                WorkItemMissing,
                400,
                "work_item_missing",
                "work_item does not exist",
            ),
            (
                WorkItemWorkspaceMismatch,
                400,
                "work_item_workspace_mismatch",
                "work_item does not belong to workspace",
            ),
            (
                PodMissing,
                400,
                "pod_missing",
                "pod does not exist or has been deleted",
            ),
            (
                PodWorkspaceMismatch,
                400,
                "pod_workspace_mismatch",
                "pod does not belong to workspace",
            ),
            (
                PodProjectMismatch,
                400,
                "pod_project_mismatch",
                "pod does not belong to issue's project",
            ),
            (
                NoPodAvailable,
                409,
                "no_pod_available",
                "no pod available; ensure the issue has a project with a default pod",
            ),
        ]
    }

    #[test]
    fn replays_all_f6_branch_triplets() {
        let branches = fixture_branches();
        assert_eq!(branches.len(), 8, "fixture branch count pinned");
        for (variant, status, code, message) in triplets() {
            assert_eq!(variant.status(), status, "{code}");
            assert_eq!(variant.code(), code);
            assert_eq!(variant.message(), message);
            // `str(exc) == message` (Python `super().__init__(message)`).
            assert_eq!(variant.to_string(), message);
            let pinned = branches
                .iter()
                .find(|branch| branch["code"] == code)
                .unwrap_or_else(|| panic!("fixture pins {code}"));
            assert_eq!(pinned["status"], status, "{code}");
            // The `when` prose quotes the message — the third field ties
            // to the fixture, not just to itself.
            let when = pinned["when"].as_str().expect("when prose");
            assert!(when.contains(message), "{code}: {when}");
        }
    }

    #[test]
    fn f6_probe_priority_and_returns_pins() {
        let section = &fixture()["validate_run_creation"];
        assert_eq!(section["source"], "runner/services/validation.py:48-182");
        // The probe pin names the exact ORM call the SQL ports.
        let probe = section["issue_probe"].as_str().expect("issue_probe prose");
        assert!(probe.contains("Issue.objects.filter(pk)"), "{probe}");
        assert!(
            probe.contains("values(workspace_id, assigned_pod_id, project_id)"),
            "{probe}"
        );
        assert!(probe.contains(".first()"), "{probe}");
        // The priority pin names every leg in order.
        let priority = section["pod_priority"]
            .as_str()
            .expect("pod_priority prose");
        for leg in [
            "explicit pod_id",
            "issue.assigned_pod_id",
            "project default pod",
            "single-project-workspace back-compat",
        ] {
            assert!(priority.contains(leg), "{leg}: {priority}");
        }
        // The unit performs no writes: the trait surface is reads-only.
        let returns = section["returns"].as_str().expect("returns prose");
        assert!(returns.contains("ValidatedRunContext"), "{returns}");
        assert!(returns.contains("NO DB writes"), "{returns}");
    }

    // ------------------------------------------------------------------
    // Behavior: a fake store drives every branch
    // ------------------------------------------------------------------

    fn test_pod(id: Uuid, workspace_id: Uuid, project_id: Uuid) -> Pod {
        let at = chrono::DateTime::from_timestamp(0, 0).expect("epoch");
        Pod {
            id,
            workspace_id,
            project_id,
            name: "pod".to_string(),
            description: String::new(),
            created_by_id: None,
            is_default: false,
            deleted_at: None,
            created_at: at,
            updated_at: at,
        }
    }

    /// Configurable [`RunCreationStore`] recording every call.
    struct FakeStore {
        role: Option<i32>,
        issue: Option<IssueProbe>,
        pods: HashMap<Uuid, Pod>,
        defaults: HashMap<Uuid, Pod>,
        project_ids: Vec<Uuid>,
        fail_with: Option<StoreError>,
        calls: RefCell<Vec<String>>,
    }

    impl FakeStore {
        fn new(role: Option<i32>) -> Self {
            Self {
                role,
                issue: None,
                pods: HashMap::new(),
                defaults: HashMap::new(),
                project_ids: Vec::new(),
                fail_with: None,
                calls: RefCell::new(Vec::new()),
            }
        }

        fn logged(&self, call: String) -> Result<(), StoreError> {
            self.calls.borrow_mut().push(call);
            match &self.fail_with {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }

        fn calls_with(&self, prefix: &str) -> Vec<String> {
            self.calls()
                .into_iter()
                .filter(|call| call.starts_with(prefix))
                .collect()
        }
    }

    impl RunCreationStore for FakeStore {
        async fn workspace_role(
            &self,
            user_id: Uuid,
            workspace_id: Uuid,
        ) -> Result<Option<i32>, StoreError> {
            self.logged(format!("role {user_id} {workspace_id}"))?;
            Ok(self.role)
        }

        async fn issue_probe(&self, work_item_id: Uuid) -> Result<Option<IssueProbe>, StoreError> {
            self.logged(format!("issue {work_item_id}"))?;
            Ok(self.issue)
        }

        async fn pod_by_id(&self, pod_id: Uuid) -> Result<Option<Pod>, StoreError> {
            self.logged(format!("pod {pod_id}"))?;
            Ok(self.pods.get(&pod_id).cloned())
        }

        async fn default_pod_for_project(
            &self,
            project_id: Uuid,
        ) -> Result<Option<Pod>, StoreError> {
            self.logged(format!("default {project_id}"))?;
            Ok(self.defaults.get(&project_id).cloned())
        }

        async fn project_ids_for_workspace(
            &self,
            workspace_id: Uuid,
        ) -> Result<Vec<Uuid>, StoreError> {
            self.logged(format!("projects {workspace_id}"))?;
            Ok(self.project_ids.clone())
        }
    }

    fn uuid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    /// Fixed ids: user . user, workspace . ws, project . project, etc.
    fn ids() -> (Uuid, Uuid, Uuid, Uuid, Uuid) {
        (uuid(1), uuid(2), uuid(3), uuid(4), uuid(5))
    }

    #[tokio::test]
    async fn missing_workspace_is_400_with_no_reads() {
        let store = FakeStore::new(Some(20));
        let (user, _, _, _, _) = ids();
        let error = validate_run_creation(&store, Some(user), None, None, None)
            .await
            .expect_err("falsy workspace rejects");
        assert_eq!(
            error,
            ValidationError::Invalid(RunCreationError::WorkspaceRequired)
        );
        assert!(store.calls().is_empty(), "no reads before the gate");
    }

    #[tokio::test]
    async fn anonymous_user_is_403_with_no_query() {
        let store = FakeStore::new(Some(20));
        let (_, ws, _, _, _) = ids();
        let error = validate_run_creation(&store, None, Some(ws), None, None)
            .await
            .expect_err("anonymous caller rejects");
        assert_eq!(
            error,
            ValidationError::Invalid(RunCreationError::NotWorkspaceMember)
        );
        assert!(store.calls().is_empty(), "None caller short-circuits");
    }

    #[tokio::test]
    async fn non_member_is_403() {
        let store = FakeStore::new(None);
        let (user, ws, _, _, _) = ids();
        let error = validate_run_creation(&store, Some(user), Some(ws), None, None)
            .await
            .expect_err("non-member rejects");
        assert_eq!(
            error,
            ValidationError::Invalid(RunCreationError::NotWorkspaceMember)
        );
    }

    #[tokio::test]
    async fn guest_role_passes_the_membership_gate() {
        // The kernel counts any role (Guest=5 included); the request then
        // proceeds to pod resolution and 409s on the empty store — proof
        // the gate passed rather than rejected.
        let store = FakeStore::new(Some(5));
        let (user, ws, _, _, _) = ids();
        let error = validate_run_creation(&store, Some(user), Some(ws), None, None)
            .await
            .expect_err("empty store 409s");
        assert_eq!(
            error,
            ValidationError::Invalid(RunCreationError::NoPodAvailable)
        );
    }

    #[tokio::test]
    async fn work_item_branches() {
        let (user, ws, project, item, _) = ids();
        // Missing row.
        let store = FakeStore::new(Some(15));
        let error = validate_run_creation(&store, Some(user), Some(ws), Some(item), None)
            .await
            .expect_err("missing work item rejects");
        assert_eq!(
            error,
            ValidationError::Invalid(RunCreationError::WorkItemMissing)
        );
        // Wrong workspace.
        let mut store = FakeStore::new(Some(15));
        store.issue = Some(IssueProbe {
            workspace_id: uuid(99),
            assigned_pod_id: None,
            project_id: Some(project),
        });
        let error = validate_run_creation(&store, Some(user), Some(ws), Some(item), None)
            .await
            .expect_err("foreign work item rejects");
        assert_eq!(
            error,
            ValidationError::Invalid(RunCreationError::WorkItemWorkspaceMismatch)
        );
    }

    #[tokio::test]
    async fn explicit_pod_branches_are_strict() {
        let (user, ws, project, item, pod_id) = ids();
        let with_issue = |store: &mut FakeStore| {
            store.issue = Some(IssueProbe {
                workspace_id: ws,
                assigned_pod_id: None,
                project_id: Some(project),
            });
        };
        // Missing (soft-deleted reads as missing via the SQL scope).
        let mut store = FakeStore::new(Some(15));
        with_issue(&mut store);
        let error = validate_run_creation(&store, Some(user), Some(ws), Some(item), Some(pod_id))
            .await
            .expect_err("missing pod rejects");
        assert_eq!(
            error,
            ValidationError::Invalid(RunCreationError::PodMissing)
        );
        // Wrong workspace.
        let mut store = FakeStore::new(Some(15));
        with_issue(&mut store);
        store
            .pods
            .insert(pod_id, test_pod(pod_id, uuid(99), project));
        let error = validate_run_creation(&store, Some(user), Some(ws), Some(item), Some(pod_id))
            .await
            .expect_err("foreign pod rejects");
        assert_eq!(
            error,
            ValidationError::Invalid(RunCreationError::PodWorkspaceMismatch)
        );
        // Wrong project (project known).
        let mut store = FakeStore::new(Some(15));
        with_issue(&mut store);
        store.pods.insert(pod_id, test_pod(pod_id, ws, uuid(98)));
        let error = validate_run_creation(&store, Some(user), Some(ws), Some(item), Some(pod_id))
            .await
            .expect_err("cross-project pod rejects");
        assert_eq!(
            error,
            ValidationError::Invalid(RunCreationError::PodProjectMismatch)
        );
        // Success: no project anchor, no work item — the project check is
        // skipped and the pod resolves.
        let mut store = FakeStore::new(Some(15));
        store.pods.insert(pod_id, test_pod(pod_id, ws, uuid(98)));
        let ctx = validate_run_creation(&store, Some(user), Some(ws), None, Some(pod_id))
            .await
            .expect("explicit pod resolves without a project anchor");
        assert_eq!(ctx.pod.id, pod_id);
        assert_eq!(ctx.work_item_id, None);
    }

    #[tokio::test]
    async fn explicit_pod_wins_and_short_circuits() {
        let (user, ws, project, item, pod_id) = ids();
        let pinned = uuid(6);
        let mut store = FakeStore::new(Some(15));
        store.issue = Some(IssueProbe {
            workspace_id: ws,
            assigned_pod_id: Some(pinned),
            project_id: Some(project),
        });
        store.pods.insert(pod_id, test_pod(pod_id, ws, project));
        store.pods.insert(pinned, test_pod(pinned, ws, project));
        store
            .defaults
            .insert(project, test_pod(uuid(7), ws, project));
        let ctx = validate_run_creation(&store, Some(user), Some(ws), Some(item), Some(pod_id))
            .await
            .expect("explicit pod resolves");
        assert_eq!(ctx.pod.id, pod_id);
        // Only the explicit leg reads: pinned/default/back-compat never run.
        assert_eq!(store.calls_with("pod "), vec![format!("pod {pod_id}")]);
        assert!(store.calls_with("default ").is_empty());
        assert!(store.calls_with("projects ").is_empty());
    }

    #[tokio::test]
    async fn pinned_pod_accepted_when_consistent() {
        let (user, ws, project, item, _) = ids();
        let pinned = uuid(6);
        let mut store = FakeStore::new(Some(15));
        store.issue = Some(IssueProbe {
            workspace_id: ws,
            assigned_pod_id: Some(pinned),
            project_id: Some(project),
        });
        store.pods.insert(pinned, test_pod(pinned, ws, project));
        let ctx = validate_run_creation(&store, Some(user), Some(ws), Some(item), None)
            .await
            .expect("pinned pod resolves");
        assert_eq!(ctx.pod.id, pinned);
        assert!(store.calls_with("default ").is_empty());
    }

    #[tokio::test]
    async fn stale_pin_falls_through_to_project_default() {
        let (user, ws, project, item, _) = ids();
        let default_id = uuid(7);
        for stale in [
            // Soft-deleted pin (reads as missing).
            None,
            // Pin in another workspace — lenient, no 400.
            Some(test_pod(uuid(6), uuid(99), project)),
            // Pin in another project — lenient, no 400.
            Some(test_pod(uuid(6), ws, uuid(98))),
        ] {
            let mut store = FakeStore::new(Some(15));
            store.issue = Some(IssueProbe {
                workspace_id: ws,
                assigned_pod_id: Some(uuid(6)),
                project_id: Some(project),
            });
            if let Some(pin) = stale {
                store.pods.insert(uuid(6), pin);
            }
            store
                .defaults
                .insert(project, test_pod(default_id, ws, project));
            let ctx = validate_run_creation(&store, Some(user), Some(ws), Some(item), None)
                .await
                .expect("stale pin falls through to the default");
            assert_eq!(ctx.pod.id, default_id);
        }
    }

    #[tokio::test]
    async fn backcompat_legs() {
        let (user, ws, _, _, _) = ids();
        let default_id = uuid(7);
        // Exactly one project: its default resolves (no work item, no pod).
        let mut store = FakeStore::new(Some(15));
        store.project_ids = vec![uuid(3)];
        store
            .defaults
            .insert(uuid(3), test_pod(default_id, ws, uuid(3)));
        let ctx = validate_run_creation(&store, Some(user), Some(ws), None, None)
            .await
            .expect("single-project back-compat resolves");
        assert_eq!(ctx.pod.id, default_id);
        // Zero or two projects: 409.
        for project_ids in [vec![], vec![uuid(3), uuid(8)]] {
            let mut store = FakeStore::new(Some(15));
            store.project_ids = project_ids;
            store
                .defaults
                .insert(uuid(3), test_pod(default_id, ws, uuid(3)));
            let error = validate_run_creation(&store, Some(user), Some(ws), None, None)
                .await
                .expect_err("non-single project set 409s");
            assert_eq!(
                error,
                ValidationError::Invalid(RunCreationError::NoPodAvailable)
            );
        }
        // One project but no default: 409.
        let mut store = FakeStore::new(Some(15));
        store.project_ids = vec![uuid(3)];
        let error = validate_run_creation(&store, Some(user), Some(ws), None, None)
            .await
            .expect_err("missing back-compat default 409s");
        assert_eq!(
            error,
            ValidationError::Invalid(RunCreationError::NoPodAvailable)
        );
    }

    #[tokio::test]
    async fn context_echoes_verified_inputs() {
        let (user, ws, project, item, pod_id) = ids();
        let mut store = FakeStore::new(Some(20));
        store.issue = Some(IssueProbe {
            workspace_id: ws,
            assigned_pod_id: None,
            project_id: Some(project),
        });
        store.pods.insert(pod_id, test_pod(pod_id, ws, project));
        let ctx = validate_run_creation(&store, Some(user), Some(ws), Some(item), Some(pod_id))
            .await
            .expect("valid inputs resolve");
        assert_eq!(ctx.workspace_id, ws);
        assert_eq!(ctx.work_item_id, Some(item));
        assert_eq!(ctx.pod.id, pod_id);
        assert_eq!(ctx.created_by, user);
    }

    #[tokio::test]
    async fn store_errors_propagate_as_store() {
        let mut store = FakeStore::new(Some(20));
        store.fail_with = Some(StoreError::Db("connection refused".to_string()));
        let (user, ws, _, _, _) = ids();
        let error = validate_run_creation(&store, Some(user), Some(ws), None, None)
            .await
            .expect_err("store failure propagates");
        assert_eq!(
            error,
            ValidationError::Store(StoreError::Db("connection refused".to_string()))
        );
        assert_eq!(error.to_string(), "database error: connection refused");
    }
}
