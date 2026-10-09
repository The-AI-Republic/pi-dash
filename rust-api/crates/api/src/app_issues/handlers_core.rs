//! D-26 handlers A: core write/read — create / retrieve / partial_update /
//! PUT-update / destroy / bulk-delete (PIDASHCONV-651).
//!
//! Ports five units from `app/views/issue/base.py` at `Ported from`
//! `01a93e17216faea7bfc156b0f864cbbe420d1c52` (no drift: the file is
//! unchanged since that sha):
//!
//! - `IssueViewSet.create` (`:397-483`)
//! - `IssueViewSet.retrieve` (`:486-619`)
//! - `IssueViewSet.partial_update` (`:621-719`, PATCH)
//! - `IssueViewSet.update` (the DRF `UpdateModelMixin` default — PUT; the
//!   view defines no `update`, and fixture FX-ISS-14 pins PUT alongside
//!   PATCH)
//! - `IssueViewSet.destroy` (`:722-755`)
//! - `BulkDeleteIssuesEndpoint.delete` (`:786-812`)
//!
//! Routes (registered in [`super::routes`]): `POST issues/` (GET stays the
//! pilot's), `GET`/`PUT`/`PATCH`/`DELETE issues/{pk}/`, `DELETE
//! bulk-delete-issues/`. List-family reads belong to pilot 2; this module
//! extends those routes, never re-ports them.
//!
//! Layering: field validation here is the DRF `is_valid()` half (per-field
//! `to_internal_value` + model validators + `validate_<field>`); the
//! object-level `validate()` half is the merged
//! [`issue_create_validate`](pidash_services::app_issues::serializers_create::issue_create_validate)
//! kernel (PIDASHCONV-638), called with prefetched probes. Detail rendering
//! is the merged serializers-B kernel (PIDASHCONV-639) over the merged
//! queries-A statements (PIDASHCONV-648). Guards go through the F-06
//! [`decide_allow`](pidash_auth::permissions::allow::decide_allow) kernel
//! only. Signals become explicit calls: orchestration via 594
//! `capture_prior_state` + `fire_state_transition`. The `github_signals`
//! completion-comment leg (`trigger_completion_comment`, which enqueues
//! `post_completion_comment` when a mirrored issue transitions into a
//! completed state) is NOT wired here — no merged handler wires it, and it
//! is outside this issue's named signals scope; it needs its own issue.
//! Tasks enqueue by name through the jobs queue; unregistered names forward
//! Celery-format downstream.
//!
//! Ported bugs (also listed in the PR):
//! - create with `is_draft=true` 500s (`user_timezone_converter(None)`);
//!   the row stays created (contract `test_create_is_draft_500s`).
//! - PUT is the undecorated DRF default: any authenticated project member
//!   (including guests) may PUT; its serializer context lacks
//!   `project_id`/`allow_triage_state`, so PUT with `state_id`/`parent_id`/
//!   `estimate_point` always 400s, PUT with `assignee_ids` 400s
//!   `{"error": "The required key does not exist."}` (the `KeyError`
//!   branch); PUT ignores `label_ids` (labels are kept, verified live on
//!   both backends — there is no wipe).
//! - PUT/partial-update `completed_at` input is discarded (save() recomputes
//!   it from the state group); create without `state_id` keeps it.
//! - explicit `sort_order`/`sequence_id` on create are overwritten
//!   (sibling max + 10000 / project max + 1).
//! - `agent_executor=""` 400s `"unknown agent executor"` (ChoiceField
//!   `allow_blank` lets `""` through to `validate()`).
//! - `description_binary` input is silently dropped (read-only `ModelField`)
//!   and unknown input keys are silently ignored (DRF).
//! - bulk-delete accepts int/bool/None/braced/urn `issue_ids` items (UUID
//!   `int=` coercion / `IN (NULL)` no-match) and counts distinct rows.
//! - `204` responses carry no `Content-Type` (verified live).
//! - detail datetimes render in the request user's timezone (verified live
//!   with a non-UTC user; the create row shifts only
//!   `created_at`/`updated_at`, leaving `completed_at` UTC).
//!
//! Known gaps (documented, not stubbed):
//! - `expand=` nests on retrieve are not rendered (pilot-2's recorded gap,
//!   same rationale: the contract suite never sends `expand` here).
//! - `?filter=` is not applied to PUT's `get_object` (DRF runs
//!   `filter_queryset` on detail fetches; the suite never sends it).
//! - HTML-indexed list keys (`label_ids[0]=…`) are treated as unknown keys;
//!   repeated keys arrive as arrays via the shared body spec.
//! - the orchestration seam is live for the pre-save snapshot and the
//!   transition's state lookups; deeper driver arms (run creation, clock
//!   reconcile, scheduler, preflight) report unreachable, exactly like the
//!   D-26 archive port's review-blessed seam: with no ticking states in any
//!   reachable world the driver provably early-returns before them, and an
//!   attempt surfaces as a logged `FireOutcome::Failed`, never a 500.
//!
//! Fixture-correction note: FX-ISS-14/21's paraphrase is wrong on five
//! points; the Python source (verified live where observable) wins:
//! create's version-task `updated_issue` dumps `request.data` (not
//! `serializer.data`); neither version call passes `archived`/`deleted`
//! kwargs; no CRUD view passes `allow_triage_state`; there is no self-parent
//! ban (`validate()` has no such check — PATCHing `parent_id=self` 204s);
//! draft-create enqueues only `issue_activity` before the 500 (the
//! post-query raises before `model_activity`/version run).

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use pidash_auth::permissions::allow::{
    decide_allow, AllowFacts, AllowLevel, AllowSpec, CreatorGate,
};
use pidash_auth::permissions::membership::ProjectRoleFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_auth::scope::TenantScope;
use pidash_db::app_pages::strip::sync_description_stripped;
use pidash_db::tasks_ticker::models::issue_agent_ticker::{
    IssueAgentTicker, COLUMNS as TICKER_COLUMNS, TABLE as TICKER_TABLE,
};
use pidash_jobs::celery::CeleryTaskMessage;
use pidash_jobs::queue::{self, NewJob};
use pidash_services::app_issues::serializers_create::{
    assignee_member_filter_sql, issue_create_to_representation, issue_create_validate,
    label_filter_null_project_sql, label_filter_sql, m2m_batches, m2m_insert_sql,
    CreateValidateError, ExecutorPolicy, IssueCreateRow, LockedIssueAttrs, LockedIssueValues,
    PodRef, ResolvedProject, ValidateAttrs, ValidateContext, ValidateInstance, ValidateProbes,
    ValidatedAttrs, DEFAULT_ASSIGNEE_EXISTS_SQL, ESTIMATE_EXISTS_SQL, HAS_ACTIVE_RUN_SQL,
    PARENT_EXISTS_SQL, POD_FETCH_SQL, PROJECT_FETCH_SQL, STATE_EXISTS_SQL,
};
use pidash_services::app_issues::serializers_detail::{
    issue_detail_to_representation, serialize_drf_datetime, serialize_iso_datetime,
    AgentLiveStateRow, AgentRunDetailRow, AgentTickerInput, IssueDetailBaseRow, IssueDetailRow,
    ACTIVE_AGENT_RUN_SQL, AGENT_RUN_COUNT_SQL, LATEST_AGENT_RUN_SQL,
};
use pidash_services::app_issues::serializers_engage::{
    GITHUB_ISSUE_SYNC_PROBE_SQL, GIT_ISSUE_SYNC_PROBE_SQL,
};
use pidash_services::auth_session::shapes::{base_host, HostSettings};
use pidash_services::dispatch::admission::{
    managed_runner_availability, LlmProfile, ENROLLED_MANAGED_RUNNERS_EXISTS_SQL,
    HEARTBEAT_GRACE_SECS, ONLINE_MANAGED_RUNNER_SQL,
};
use pidash_services::dispatch::policy::{
    cloud_agent_is_configured, managed_runner_is_enabled, UserFlags,
};
use pidash_services::orchestration::blockers::{
    has_open_blockers_sql, relations_summary, summary_sql, BlockerRow,
};
use pidash_services::orchestration::clock::ProjectClockPolicy;
use pidash_services::orchestration::creation::{
    AdmissionError, CreationError, CreationSeam, ExecutionError, ExecutionFields, ExecutionRequest,
    FinalizeAgentRunSeam, IssueView, LockedIssue, NewAgentRun, PodView, ProjectView, RenderBundle,
    RenderedTurn, RunView, RunnerView, StateView, STATE_SELECT_SQL,
};
use pidash_services::orchestration::entries::{
    capture_prior_state, fire_state_transition, EntriesSeam, FireOutcome, FireRequest,
    PreflightSeam, PRIOR_STATE_SELECT_SQL,
};
use pidash_services::prompting::composer::OverrideRow;
use pidash_types::dispatch::{AgentExecutorKind, ManagedRunnerReason};
use pidash_types::orchestration::StateRef;
use pidash_types::WorkspaceId;

use super::queries_core::{
    attachment_count_select, link_count_select, sub_issues_count_select,
    ASSIGNEE_IDS_ACTIVE_SELECT, CYCLE_ID_SELECT, ISSUE_COLUMNS, LABEL_IDS_SELECT,
    MODULE_IDS_SELECT,
};
use super::{
    actor_user_id, json_response, resolve_project_id, Denial, HandlerResult, NOT_FOUND_BODY,
};
use crate::middleware::SessionHandle;
use crate::serializer::render_datetime_in;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json_cpython::{
    parse_request_data, to_serde_publish, JsonFail, JSON_PARSE_PREFIX,
};

/// Detail path in `app/urls/issue.py:81-84` form.
pub const CORE_DETAIL_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/";
/// Bulk-delete path in `app/urls/issue.py:93-95` form.
pub const BULK_DELETE_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/bulk-delete-issues/";

/// DRF `get_object_or_404` on PUT (`django/http/shortcuts.py`): the
/// `Http404` carries the model message, which DRF's `exception_handler`
/// maps to `NotFound(*args)` — verified live by hexdump (lowercase
/// `detail`, like every other DRF exception body).
const PUT_MISSING_BODY: &str = r#"{"detail":"No Issue matches the given query."}"#;
/// `handle_exception`'s `KeyError` branch (PUT `assignee_ids` — verified live).
const KEY_ERROR_BODY: &str = r#"{"error":"The required key does not exist."}"#;
/// `partial_update`'s inline missing-issue body (`base.py:668`).
const ISSUE_NOT_FOUND_BODY: &str = r#"{"error":"Issue not found"}"#;
/// Retrieve's guest-view body (`base.py:604-607`).
const GUEST_VIEW_BODY: &str = r#"{"error":"You are not allowed to view this issue"}"#;
/// Bulk-delete's empty-ids body (`base.py:792-794`).
const ISSUE_IDS_REQUIRED_BODY: &str = r#"{"error":"Issue IDs are required"}"#;
/// Destroy's synced-issue guard (`base.py:731-737`).
const DESTROY_SYNCED_BODY: &str = r#"{"error":"This issue is synced from a Git provider. Unbind the project's repository to delete."}"#;

/// The create post-query's 26 `.values()` keys in WIRE order (captured raw
/// from live creates — Django emits the concrete fields first in compiler
/// order, then the annotations in annotation-definition order; this is NOT
/// the `.values()` call order FX-ISS-14 lists, and live wins).
const CREATE_RESPONSE_FIELDS: [&str; 26] = [
    "id",
    "name",
    "state_id",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "is_draft",
    "archived_at",
    "deleted_at",
    "cycle_id",
    "link_count",
    "attachment_count",
    "sub_issues_count",
    "assignee_ids",
    "label_ids",
    "module_ids",
];

/// Body spec for the shared DRF body pipeline: the two `ListField`s arrive
/// as arrays; every other key arrives as its last value. `skip_blank_fields`
/// is empty on purpose: the per-field HTML `get_value` rules below need the
/// raw `''` (allow-null-to-None vs skip vs keep differ per field).
const CORE_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &["assignee_ids", "label_ids"],
    skip_blank_fields: &[],
};

/// `X-Pi-Dash-Skip-Immediate-Dispatch: 1` (`base.py:677`).
const SKIP_DISPATCH_HEADER: &str = "x-pi-dash-skip-immediate-dispatch";

/// Celery task names (plain `@shared_task`: module + function).
const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";
const MODEL_ACTIVITY_TASK: &str = "pi_dash.bgtasks.webhook_task.model_activity";
const RECENT_VISITED_TASK: &str = "pi_dash.bgtasks.recent_visited_task.recent_visited_task";
const VERSION_TASK: &str =
    "pi_dash.bgtasks.issue_description_version_task.issue_description_version_task";
const SOFT_DELETE_TASK: &str = "pi_dash.bgtasks.deletion_task.soft_delete_related_objects";

/// `now()` truncated to microseconds: Postgres `timestamptz` keeps micros,
/// and two successive `now()` calls must not drift on the sub-micro tail.
fn utc_now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    now.with_nanosecond(now.nanosecond() / 1000 * 1000)
        .unwrap_or(now)
}

// ---------------------------------------------------------------------------
// Gate (F-06 only) + tenant facts
// ---------------------------------------------------------------------------

/// Per-route `@allow_permission` shape: the allowed roles plus whether the
/// `creator=True, model=Issue` bypass is live.
struct RouteGate {
    roles: &'static [i32],
    creator_bypass: bool,
}

const GATE_CREATE: RouteGate = RouteGate {
    roles: &[ROLE_ADMIN, ROLE_MEMBER],
    creator_bypass: false,
};
const GATE_RETRIEVE: RouteGate = RouteGate {
    roles: &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST],
    creator_bypass: true,
};
const GATE_PATCH: RouteGate = RouteGate {
    roles: &[ROLE_ADMIN, ROLE_MEMBER],
    creator_bypass: true,
};
const GATE_DESTROY: RouteGate = RouteGate {
    roles: &[ROLE_ADMIN],
    creator_bypass: true,
};
const GATE_BULK_DELETE: RouteGate = RouteGate {
    roles: &[ROLE_ADMIN],
    creator_bypass: false,
};

/// Facts every core handler needs after the gate: ids, the actor's
/// timezone + flags, and the project row's consumed columns.
struct CoreTenant {
    project_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    timezone: Tz,
    user_active: bool,
    user_bot: bool,
    default_assignee_id: Option<Uuid>,
    guest_view_all_features: bool,
}

/// The session half of Django's auth: no session / key / UUID id means
/// anonymous → 401. The identifier rewrite runs next (it skips anonymous
/// callers, who already 401'd here), and only then the user row + tz.
fn core_session(
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Uuid, Denial> {
    actor_user_id(extension).ok_or(Denial::Unauthorized)
}

/// The user half: the row (a session whose user is gone authenticates as
/// anonymous → 401), then the timezone activation (`TimezoneMixin.initial`
/// runs before the decorator, so an unparseable timezone 500s ahead of
/// any 403).
async fn core_user(pool: &sqlx::PgPool, user_id: &Uuid) -> Result<(Tz, bool, bool), Denial> {
    let user: Option<(String, bool, bool)> =
        sqlx::query_as(r#"SELECT user_timezone, is_active, is_bot FROM users WHERE id = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let (timezone_name, user_active, user_bot) = user.ok_or(Denial::Unauthorized)?;
    let timezone: Tz = timezone_name.parse().map_err(|_| Denial::ServerError)?;
    Ok((timezone, user_active, user_bot))
}

/// How the view body checks the project row: create's
/// `Project.objects.get(pk)` has no slug filter; retrieve's adds
/// `workspace__slug`; PATCH/PUT/bulk/destroy never fetch the project (their
/// lookups 404/empty on their own terms).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProjectCheck {
    None,
    AnyWorkspace,
    InSlug,
}

/// The gate order is Django's `initial()` + decorator order, and the 656
/// review's defect 3 runs it too: session 401 → identifier-rewrite 404 →
/// user-row 401 / tz-activation 500 → decorator 403 → tenant facts.
/// Unknown slugs 403 here (no membership row can match), never 500.
async fn core_gate(
    pool: &sqlx::PgPool,
    slug: &str,
    project_raw: &str,
    user_id: &Uuid,
    gate: &RouteGate,
    pk: Option<Uuid>,
    project_check: ProjectCheck,
) -> Result<CoreTenant, Denial> {
    let project_id = resolve_project_id(pool, slug, project_raw).await?;
    let (timezone, user_active, user_bot) = core_user(pool, user_id).await?;
    let scope = TenantScope::new(WorkspaceId::from(slug));
    let facts = allow_facts(pool, slug, &project_id, user_id, gate, pk)
        .await
        .map_err(|_| Denial::ServerError)?;
    let spec = AllowSpec {
        level: AllowLevel::Project,
        creator_gate: CreatorGate::App,
        creator_bypass: gate.creator_bypass,
    };
    if !decide_allow(&spec, &scope, &facts) {
        return Err(Denial::Forbidden);
    }
    tenant_facts(pool, slug, &project_id, user_id, project_check)
        .await
        .map(|tenant| CoreTenant {
            project_id: tenant.0,
            workspace_id: tenant.1,
            user_id: *user_id,
            timezone,
            user_active,
            user_bot,
            default_assignee_id: tenant.2,
            guest_view_all_features: tenant.3,
        })
}

/// One `EXISTS` per decorator branch (`app/permissions/base.py:20-80`):
/// separate exists-queries (not a single role fetch) so multi-row
/// memberships decide exactly like the decorator's `role__in` filters.
/// `assignee__member_project` spans aside, every membership read here is
/// soft-delete scoped (`SoftDeletionManager`).
async fn allow_facts(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
    gate: &RouteGate,
    pk: Option<Uuid>,
) -> Result<AllowFacts, sqlx::Error> {
    let flip = |row: Option<(i32,)>| row.is_some();
    let member: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2
           AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await?;
    let admin: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2 AND wm.role = $3
           AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .bind(ROLE_ADMIN)
    .fetch_optional(pool)
    .await?;
    let any_project: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await?;
    let allowed_project: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.role = ANY($4) AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .bind(
        gate.roles
            .iter()
            .map(|role| *role as i16)
            .collect::<Vec<i16>>(),
    )
    .fetch_optional(pool)
    .await?;
    let creator = match (gate.creator_bypass, pk) {
        (true, Some(pk)) => {
            let row: Option<(i32,)> = sqlx::query_as(
                r#"SELECT 1 FROM issues
                   WHERE id = $1 AND created_by_id = $2 AND deleted_at IS NULL"#,
            )
            .bind(pk)
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
            flip(row)
        }
        _ => false,
    };
    Ok(AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: flip(member),
        has_allowed_workspace_role: false,
        is_creator: creator,
        has_allowed_project_role: flip(allowed_project),
        is_project_member: flip(any_project),
        is_workspace_admin: flip(admin),
    })
}

/// View-body tenant facts: the project row per [`ProjectCheck`] (a miss
/// is the `ObjectDoesNotExist` 404), plus the consumed columns. With
/// [`ProjectCheck::None`] no project row is read (those paths must 404 on
/// their own lookups instead); the zero workspace id is a placeholder the
/// callers overwrite from their looked-up issue row before any use.
async fn tenant_facts(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    _user_id: &Uuid,
    check: ProjectCheck,
) -> Result<(Uuid, Uuid, Option<Uuid>, bool), Denial> {
    if check == ProjectCheck::None {
        return Ok((*project_id, Uuid::nil(), None, false));
    }
    let project: Option<(Uuid, Option<Uuid>, bool)> = if check == ProjectCheck::InSlug {
        sqlx::query_as(
            r#"SELECT p.workspace_id, p.default_assignee_id, p.guest_view_all_features
               FROM projects p JOIN workspaces w ON w.id = p.workspace_id
               WHERE p.id = $1 AND w.slug = $2 AND p.deleted_at IS NULL"#,
        )
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    } else {
        sqlx::query_as(
            r#"SELECT workspace_id, default_assignee_id, guest_view_all_features FROM projects
               WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    };
    let (workspace_id, default_assignee_id, guest_view) = project.ok_or(Denial::NotFound)?;
    Ok((*project_id, workspace_id, default_assignee_id, guest_view))
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// `base_host(request, is_app=True)` over the resolved settings URLs: unset
/// both → `ImproperlyConfigured` → the generic 500.
fn request_origin(state: &AppState) -> Result<String, Denial> {
    let settings = host_settings_of(state);
    let origin = base_host(&settings, false, false, true);
    if origin.is_empty() {
        return Err(Denial::ServerError);
    }
    Ok(origin)
}

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// 204 with NO `Content-Type`: Django strips it on empty responses
/// (verified live — `ct=-` on PATCH/DELETE 204s).
fn empty_response() -> Response {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("204 response")
}

fn not_found_detail(body: &str, status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_owned()))
        .expect("detail response")
}

/// CPython `json.dumps` spacing (`", "` / `": "`) over a serde value: the
/// `requested_data` / `current_instance` / `updated_issue` kwargs. ASCII
/// escaping is intentionally NOT replicated — the queue row stores the
/// parsed value, so only the value must match (same rationale as the D-26
/// archive port's `to_spaced_json`).
struct SpacedFormatter;

impl serde_json::ser::Formatter for SpacedFormatter {
    fn begin_object_key<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_value<W>(&mut self, writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        writer.write_all(b": ")
    }

    fn begin_array_value<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }
}

/// Serialize with [`SpacedFormatter`] (non-ASCII stays literal: the queue
/// transport re-encodes anyway, so only the parsed value must match).
fn to_spaced_json<T: serde::Serialize>(value: &T) -> Result<String, Denial> {
    let mut buf = Vec::new();
    let mut ser = serde_json::ser::Serializer::with_formatter(&mut buf, SpacedFormatter);
    value.serialize(&mut ser).map_err(|_| Denial::ServerError)?;
    String::from_utf8(buf).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Request bodies (DRF `request.data`)
// ---------------------------------------------------------------------------

/// Parsed `request.data`: the JSON value (or form text map as an object),
/// uploads per key, and whether HTML-input `get_value` rules apply.
struct RequestData {
    value: Value,
    files: BTreeMap<String, Vec<shared_body::FilePart>>,
    is_html: bool,
}

fn unsupported_media_type(message: String) -> Response {
    let body = format!(
        "{{\"detail\":{}}}",
        serde_json::to_string(&message).expect("415 body")
    );
    Response::builder()
        .status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("415 response")
}

/// Every core write path parses through the shared DRF body pipeline:
/// empty-by-Content-Length is `{}`, unsupported/missing content type is the
/// 415, JSON text runs the CPython-error parser, forms run the shared
/// HTML-input machine. `ParseDetail` maps through `Denial::BadDetail`
/// (lowercase `detail`, hexdump-verified).
#[allow(clippy::result_large_err)]
fn negotiate_input(headers: &HeaderMap, body: &[u8]) -> Result<RequestData, Response> {
    let map_error = |error: shared_body::BodyError| match error {
        shared_body::BodyError::UnsupportedMediaType(message) => unsupported_media_type(message),
        shared_body::BodyError::ParseDetail(message) => Denial::BadDetail(message).into_response(),
        shared_body::BodyError::ServerError => Denial::ServerError.into_response(),
    };
    match shared_body::negotiate_body(headers, body, &CORE_BODY_SPEC).map_err(map_error)? {
        shared_body::NegotiatedBody::Empty => Ok(RequestData {
            value: Value::Object(Map::new()),
            files: BTreeMap::new(),
            is_html: false,
        }),
        shared_body::NegotiatedBody::JsonText { text, .. } => parse_request_data(text.as_bytes())
            .map(|parsed| RequestData {
                value: to_serde_publish(&parsed),
                files: BTreeMap::new(),
                is_html: false,
            })
            .map_err(|fail| match fail {
                JsonFail::Message(detail) => {
                    Denial::BadDetail(format!("{JSON_PARSE_PREFIX}{detail}")).into_response()
                }
                JsonFail::Recursion => Denial::ServerError.into_response(),
            }),
        shared_body::NegotiatedBody::Form { map, files, .. } => Ok(RequestData {
            value: Value::Object(map),
            files,
            is_html: true,
        }),
    }
}

/// One input value for field `get_value`: JSON/form scalars resolve to
/// `Json`, missing keys to `Missing`, and keys carrying any upload to
/// `Files` (DRF merges files into `request.data`, so a files-only key is
/// present and reads as the file object — last file wins for scalars,
/// text-then-file order for `getlist`, which only the two `ListField`s
/// use).
enum InputRef<'a> {
    Missing,
    Json(&'a Value),
    Files,
}

impl RequestData {
    /// `key in request.data` over the merged text+files map. Files-only
    /// keys count as present (DRF merges files into `request.data`).
    fn contains(&self, key: &str) -> bool {
        self.value
            .as_object()
            .is_some_and(|map| map.contains_key(key))
            || self.files.contains_key(key)
    }

    fn input(&self, key: &str) -> InputRef<'_> {
        if let Some(files) = self.files.get(key) {
            if !files.is_empty() {
                return InputRef::Files;
            }
        }
        match self.value.as_object().and_then(|map| map.get(key)) {
            Some(value) => InputRef::Json(value),
            None => InputRef::Missing,
        }
    }

    /// `request.data.get(key) is not None`: files count as set (DRF's
    /// `.get` returns the file object, never `None`, for present keys).
    fn get_is_set(&self, key: &str) -> bool {
        match self.input(key) {
            InputRef::Missing => false,
            InputRef::Json(Value::Null) => false,
            InputRef::Json(_) | InputRef::Files => true,
        }
    }

    /// `request.data.pop(key, default)`: JSON pops the value; HTML
    /// (`QueryDict.pop`) returns the value LIST — always truthy when the
    /// key is present, even for `skip_activity=0`.
    fn pop_skip_activity(&mut self) -> bool {
        if self.is_html {
            let present = self.contains("skip_activity");
            if let Some(map) = self.value.as_object_mut() {
                map.remove("skip_activity");
            }
            return present;
        }
        let Some(map) = self.value.as_object_mut() else {
            return false;
        };
        let value = map.remove("skip_activity").unwrap_or(Value::Null);
        json_is_truthy(&value)
    }
}

/// Python truthiness over parsed JSON (`request.data.pop(...) and ...`):
/// `false`/`null`/`0`/`""`/`[]`/`{}` are falsy, everything else truthy.
fn json_is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().is_some_and(|float| float != 0.0)
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

// ---------------------------------------------------------------------------
// DRF field validation (`is_valid()` before `validate()`)
// ---------------------------------------------------------------------------

/// DRF 3.15.2 message texts (`fields.py`, verified live).
const MSG_REQUIRED: &str = "This field is required.";
const MSG_NULL: &str = "This field may not be null.";
const MSG_BLANK: &str = "This field may not be blank.";
const MSG_INVALID_STR: &str = "Not a valid string.";
const MSG_NULL_CHARACTERS: &str = "Null characters are not allowed.";
const MSG_INVALID_INT: &str = "A valid integer is required.";
const MSG_INVALID_FLOAT: &str = "A valid number is required.";
const MSG_INVALID_BOOL: &str = "Must be a valid boolean.";
const MSG_STRING_TOO_LARGE: &str = "String value too large.";
const MSG_DATE_INVALID: &str =
    "Date has wrong format. Use one of these formats instead: YYYY-MM-DD.";
const MSG_DATETIME_INVALID: &str = "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";
const MSG_DATETIME_OVERFLOW: &str = "Datetime value out of range.";
const MSG_JSON_INVALID: &str = "Value must be valid JSON.";
/// DRF `IntegerField`/`FloatField.MAX_STRING_LENGTH` (chars, not bytes).
const MAX_STRING_LENGTH: usize = 1000;

/// `CharField.to_internal_value` (`fields.py`): bools and composites fail;
/// str/int/float coerce via `str()` (ints/floats stringify — `name=123`
/// stores `"123"`, verified live), then whitespace-trimmed.
fn parse_char(value: &Value, max_length: Option<usize>) -> Result<String, Vec<String>> {
    let text = match value {
        Value::Bool(_) => return Err(vec![MSG_INVALID_STR.to_owned()]),
        Value::String(text) => text.clone(),
        Value::Number(number) => json_number_str(number),
        Value::Null | Value::Array(_) | Value::Object(_) => {
            return Err(vec![MSG_INVALID_STR.to_owned()])
        }
    };
    let trimmed = text.trim().to_owned();
    if let Some(max) = max_length {
        if trimmed.chars().count() > max {
            return Err(vec![format!(
                "Ensure this field has no more than {max} characters."
            )]);
        }
    }
    // `ProhibitNullCharactersValidator` (runs after `MaxLengthValidator`,
    // before model validators): without it a NUL would reach Postgres,
    // which rejects it with a 500 instead of Django's 400.
    if trimmed.contains('\0') {
        return Err(vec![MSG_NULL_CHARACTERS.to_owned()]);
    }
    Ok(trimmed)
}

/// `ChoiceField.to_internal_value`: `""` passes only with `allow_blank`;
/// lookup is over `str(data)`; failures echo the raw input.
fn parse_choice(value: &Value, choices: &[&str], allow_blank: bool) -> Result<String, Vec<String>> {
    if let Value::String(text) = value {
        if text.is_empty() && allow_blank {
            return Ok(String::new());
        }
    }
    // `str(data)` for the lookup and the echo: JSON scalars stringify;
    // composites use the CPython `repr` (a dict input echoes as
    // `"{'a': 1}"`).
    let key = choice_stringify(value);
    if choices.contains(&key.as_str()) {
        return Ok(key);
    }
    Err(vec![format!("\"{key}\" is not a valid choice.")])
}

/// `int(re_decimal.sub('', str(data)))` with `re_decimal = /\.0*\s*$/`
/// (`fields.py:592`): `"5.0"`/`5.0` → 5 (via the `str()` round-trip —
/// exponent-spelled floats like `1e16` stringify to `"1e+16"` and fail,
/// exactly like CPython); `"5.5"`/bools/composites → invalid. Out-of-`i64`
/// magnitudes are NOT invalid: Python ints are unbounded, so they flow to
/// the field's `min_value`/`max_value` validators.
fn parse_integer(value: &Value) -> Result<IntegerValue, Vec<String>> {
    match value {
        Value::Bool(_) => Err(vec![MSG_INVALID_INT.to_owned()]),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return Ok(IntegerValue::Int(int));
            }
            if let Some(uint) = number.as_u64() {
                if let Ok(int) = i64::try_from(uint) {
                    return Ok(IntegerValue::Int(int));
                }
                return Ok(IntegerValue::TooBig);
            }
            // Int-shaped literals past `u64`/`i64` are out of range (never
            // `invalid` — CPython ints are unbounded, so DRF reports
            // min/max); only float shapes fall through to `repr`.
            if let Some(literal) = int_literal(number) {
                return Ok(if literal.starts_with('-') {
                    IntegerValue::TooSmall
                } else {
                    IntegerValue::TooBig
                });
            }
            match number.as_f64() {
                Some(float) => {
                    let text = strip_decimal_zeros(&py_float_str(float));
                    match parse_python_int(&text) {
                        Some(int) => Ok(IntegerValue::Int(int)),
                        None if is_big_int_spelling(&text) => Ok(IntegerValue::TooBig),
                        None if is_small_int_spelling(&text) => Ok(IntegerValue::TooSmall),
                        None => Err(vec![MSG_INVALID_INT.to_owned()]),
                    }
                }
                None => Err(vec![MSG_INVALID_INT.to_owned()]),
            }
        }
        Value::String(text) => {
            if text.chars().count() > MAX_STRING_LENGTH {
                return Err(vec![MSG_STRING_TOO_LARGE.to_owned()]);
            }
            let stripped = strip_decimal_zeros(text);
            match parse_python_int(&stripped) {
                Some(int) => Ok(IntegerValue::Int(int)),
                None if is_big_int_spelling(&stripped) => Ok(IntegerValue::TooBig),
                None if is_small_int_spelling(&stripped) => Ok(IntegerValue::TooSmall),
                None => Err(vec![MSG_INVALID_INT.to_owned()]),
            }
        }
        Value::Null | Value::Array(_) | Value::Object(_) => Err(vec![MSG_INVALID_INT.to_owned()]),
    }
}

/// An integer input: in-range, or out-of-`i64` (the driver's range check
/// turns these into the field's `min_value`/`max_value` messages).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntegerValue {
    Int(i64),
    TooBig,
    TooSmall,
}

/// CPython `int(str)`: surrounding whitespace, one sign, digits with
/// single interior underscores (`"1_0"` → 10). Anything else → `None`.
fn parse_python_int(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if digits.is_empty() {
        return None;
    }
    let mut cleaned = String::with_capacity(digits.len());
    let mut prev_underscore = true;
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
            continue;
        }
        if !ch.is_ascii_digit() {
            return None;
        }
        prev_underscore = false;
        cleaned.push(ch);
    }
    if prev_underscore {
        return None;
    }
    let magnitude: i128 = cleaned.parse().ok()?;
    let signed = if trimmed.starts_with('-') {
        -magnitude
    } else {
        magnitude
    };
    i64::try_from(signed).ok()
}

/// A well-formed over-`i64` non-negative int spelling (→ `max_value`).
fn is_big_int_spelling(text: &str) -> bool {
    int_spelling_sign(text) == Some(false)
}

/// A well-formed under-`i64` negative int spelling (→ `min_value`).
fn is_small_int_spelling(text: &str) -> bool {
    int_spelling_sign(text) == Some(true)
}

/// `Some(negative)` when `text` is a well-formed int spelling whose
/// magnitude exceeds `i64` (interior underscores validated like
/// [`parse_python_int`); `None` for malformed spellings.
fn int_spelling_sign(text: &str) -> Option<bool> {
    let trimmed = text.trim();
    let negative = trimmed.starts_with('-');
    let digits = trimmed
        .strip_prefix(['+', '-'])
        .unwrap_or(trimmed)
        .replace('_', "");
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Underscore placement was validated loosely here; `parse_python_int`
    // already rejected the malformed ones, so any well-formed spelling that
    // failed `i64` is out of range by definition — but only call this on
    // the `None` path (see callers).
    let magnitude: i128 = digits.parse().unwrap_or(i128::MAX);
    if magnitude > i128::from(i64::MAX) {
        Some(negative)
    } else {
        None
    }
}

/// `re.sub(r'\.0*\s*$', '', text)`: strips a trailing dot plus all-zero
/// fraction (and trailing whitespace after it); anything else is
/// untouched, so `int()` still rejects `"5.5"`. Note `"5."` strips to
/// `"5"` (the `0*` matches empty) while a bare `"."` is kept (empty head).
fn strip_decimal_zeros(text: &str) -> String {
    let trimmed_end = text.trim_end();
    let Some(dot) = trimmed_end.rfind('.') else {
        return text.to_owned();
    };
    let (head, tail) = trimmed_end.split_at(dot);
    if !head.is_empty() && tail[1..].chars().all(|ch| ch == '0') {
        return head.to_owned();
    }
    text.to_owned()
}

/// `str(data)` for choice lookup/echo.
fn choice_stringify(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => json_number_str(number),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Null => "None".to_owned(),
        Value::Array(_) | Value::Object(_) => py_repr(value),
    }
}

/// Exact digits for an int-shaped JSON literal. `arbitrary_precision`
/// preserves the source spelling in [`ToString`], so literals beyond
/// `u64`/`i64` (which `as_f64` would render lossy, e.g. `"1e+23"`) echo
/// exactly like CPython's `str(int)`.
fn int_literal(number: &serde_json::Number) -> Option<String> {
    // In-range magnitudes format numerically (`-0` → `"0"`, like CPython).
    if let Some(int) = number.as_i64() {
        return Some(int.to_string());
    }
    if let Some(uint) = number.as_u64() {
        return Some(uint.to_string());
    }
    // Past `i64`/`u64` the literal is exact (`arbitrary_precision`); only
    // int-shaped spellings qualify — exponent/fraction shapes are floats.
    let literal = number.to_string();
    let digits = literal.strip_prefix('-').unwrap_or(&literal);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        Some(literal)
    } else {
        None
    }
}

/// `str()` over a JSON number: ints spell exactly (even past `u64`, via
/// the preserved literal); floats use CPython `repr` ([`py_float_str`]),
/// not serde's rendering (exponent `+` and zero-padding differ: `1e+16`,
/// `1e-05`).
fn json_number_str(number: &serde_json::Number) -> String {
    if let Some(literal) = int_literal(number) {
        return literal;
    }
    match number.as_f64() {
        Some(float) => py_float_str(float),
        None => number.to_string(),
    }
}

/// CPython `repr(float)`: shortest round-trip, lowercase `e`, always-signed
/// two-or-more-digit exponent (`1e+16`, `1e-05`), `.0` for integral values,
/// `inf`/`-inf`/`nan` spelled lowercase.
fn py_float_str(float: f64) -> String {
    if float.is_nan() {
        return "nan".to_owned();
    }
    if float.is_infinite() {
        return if float > 0.0 {
            "inf".to_owned()
        } else {
            "-inf".to_owned()
        };
    }
    let rendered = format!("{float:?}");
    let Some(exponent_at) = rendered.find('e') else {
        return rendered;
    };
    let (mantissa, exponent) = rendered.split_at(exponent_at);
    let digits = exponent[1..]
        .trim_start_matches('+')
        .trim_start_matches('-');
    let negative = exponent.contains('-');
    let padded = format!("{digits:0>2}");
    if negative {
        format!("{mantissa}e-{padded}")
    } else {
        format!("{mantissa}e+{padded}")
    }
}

/// `float(data)`: bools coerce (`True` → 1.0, verified live), strings
/// parse, everything else fails. DRF's `overflow` arm (`float(10**1000)`
/// as an int object) is unreachable: JSON ints parse to `i64`/`u64`/`f64`
/// upstream, and all three convert without raising.
fn parse_float(value: &Value) -> Result<f64, Vec<String>> {
    match value {
        Value::Bool(flag) => Ok(if *flag { 1.0 } else { 0.0 }),
        Value::Number(number) => number
            .as_f64()
            .ok_or_else(|| vec![MSG_INVALID_FLOAT.to_owned()]),
        Value::String(text) => {
            if text.chars().count() > MAX_STRING_LENGTH {
                return Err(vec![MSG_STRING_TOO_LARGE.to_owned()]);
            }
            parse_python_float(text).ok_or_else(|| vec![MSG_INVALID_FLOAT.to_owned()])
        }
        Value::Null | Value::Array(_) | Value::Object(_) => Err(vec![MSG_INVALID_FLOAT.to_owned()]),
    }
}

/// CPython `float(str)`: surrounding whitespace tolerated, `inf`/`nan`
/// (any case, optional sign) accepted. Underscores (`"1_0"`) are accepted
/// by CPython too — matched here.
fn parse_python_float(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lowered = trimmed.to_lowercase();
    match lowered.as_str() {
        "inf" | "+inf" | "infinity" | "+infinity" => return Some(f64::INFINITY),
        "-inf" | "-infinity" => return Some(f64::NEG_INFINITY),
        "nan" | "+nan" | "-nan" => return Some(f64::NAN),
        _ => {}
    }
    // CPython allows a single underscore only between two ASCII digits
    // (`"1_0"` ok; `"1__0"`, `"_1"`, `"1_"`, `"1_e5"` fail).
    if trimmed.contains('_') {
        let chars: Vec<char> = trimmed.chars().collect();
        for (index, ch) in chars.iter().enumerate() {
            if *ch == '_' {
                let left = index > 0 && chars[index - 1].is_ascii_digit();
                let right = index + 1 < chars.len() && chars[index + 1].is_ascii_digit();
                if !(left && right) {
                    return None;
                }
            }
        }
    }
    let deunderscored = trimmed.replace('_', "");
    // `float(str)` never raises `OverflowError` (only `float(huge_int)`
    // does, and JSON ints arrive via the `Number` arm): over-long digit
    // strings are already rejected by `MAX_STRING_LENGTH` above, and
    // `"1e999"`-style overflow parses to `inf`, exactly like CPython.
    deunderscored.parse::<f64>().ok()
}

/// `BooleanField.to_internal_value`: the `TRUE_VALUES`/`FALSE_VALUES` sets
/// (case-insensitive for strings; `1.0 == 1` joins the true set by numeric
/// equality, `0.0` the false set). `allow_null` is handled by the caller
/// (`validate_empty_values` runs first).
fn parse_boolean(value: &Value) -> Result<bool, Vec<String>> {
    const TRUE_STRS: [&str; 6] = ["t", "y", "yes", "true", "on", "1"];
    const FALSE_STRS: [&str; 6] = ["f", "n", "no", "false", "off", "0"];
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int == 1 {
                    return Ok(true);
                }
                if int == 0 {
                    return Ok(false);
                }
            } else if let Some(uint) = number.as_u64() {
                if uint == 1 {
                    return Ok(true);
                }
                if uint == 0 {
                    return Ok(false);
                }
            } else if let Some(float) = number.as_f64() {
                if float == 1.0 {
                    return Ok(true);
                }
                if float == 0.0 {
                    return Ok(false);
                }
            }
            Err(vec![MSG_INVALID_BOOL.to_owned()])
        }
        Value::String(text) => {
            let lowered = text.to_lowercase();
            if TRUE_STRS.contains(&lowered.as_str()) {
                Ok(true)
            } else if FALSE_STRS.contains(&lowered.as_str()) {
                Ok(false)
            } else {
                Err(vec![MSG_INVALID_BOOL.to_owned()])
            }
        }
        Value::Null | Value::Array(_) | Value::Object(_) => Err(vec![MSG_INVALID_BOOL.to_owned()]),
    }
}

/// `JSONField.to_internal_value` (non-binary): `json.dumps` must succeed —
/// always true for parsed-JSON values (form strings pass through as-is and
/// store as JSON strings, exactly like Django).
fn parse_json_value(value: &Value) -> Result<Value, Vec<String>> {
    match value {
        Value::Null
        | Value::Bool(_)
        | Value::Number(_)
        | Value::String(_)
        | Value::Array(_)
        | Value::Object(_) => Ok(value.clone()),
    }
}

/// CPython `repr()` over parsed JSON values, for the `%s`-formatted UUID
/// and choice echoes: strings render bare (no quotes), bools/`None`
/// capitalized, floats shortest-round-trip, lists/dicts with `", "`/`": "`
/// separators and single-quoted string items. Dicts iterate in insertion
/// order (serde preserves it — `preserve_order`).
fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => json_number_str(number),
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items
                .iter()
                .map(|item| match item {
                    Value::String(text) => py_repr_string(text),
                    other => py_repr(other),
                })
                .collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| {
                    let rendered = match item {
                        Value::String(text) => py_repr_string(text),
                        other => py_repr(other),
                    };
                    format!("{}: {rendered}", py_repr_string(key))
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// CPython string `repr`: single quotes unless the text holds `'` but no
/// `"`, in which case double quotes wrap it (`repr("it's") ==
/// `"it's"`); `\n`/`\r`/`\t` escapes, backslash/quote escaping, and
/// non-printables as `\xNN`/`\uNNNN`. `str()`-style bare rendering is the
/// caller's choice (see [`py_repr`]).
fn py_repr_string(text: &str) -> String {
    // CPython `repr` picks `"` when the text contains `'` but no `"` (so
    // the inner quote needs no escape); otherwise `'`, escaping inner `'`.
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for ch in text.chars() {
        if ch == quote {
            out.push('\\');
            out.push(ch);
            continue;
        }
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() || (ch.is_whitespace() && ch != ' ') => {
                let code = ch as u32;
                if code <= 0xFF {
                    out.push_str(&format!("\\x{code:02x}"));
                } else if code <= 0xFFFF {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            ch => out.push(ch),
        }
    }
    out.push(quote);
    out
}

// ---------------------------------------------------------------------------
// DRF date/datetime input (`parse_date` / `parse_datetime` + `enforce_timezone`)
// ---------------------------------------------------------------------------

/// Django `parse_date`: `date.fromisoformat` (extended, basic, week —
/// NOT ordinal, verified live) or the 1-2-digit `date_re` fallback.
fn parse_drf_date(text: &str) -> Option<NaiveDate> {
    if let Some(date) = parse_iso_date_part(text) {
        return Some(date);
    }
    // `date_re`: `(?P<year>\d{4})-(?P<month>\d{1,2})-(?P<day>\d{1,2})$`.
    let mut parts = text.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || year.len() != 4 {
        return None;
    }
    if !(1..=2).contains(&month.len()) || !(1..=2).contains(&day.len()) {
        return None;
    }
    NaiveDate::from_ymd_opt(
        parse_digits(year)? as i32,
        parse_digits(month)?,
        parse_digits(day)?,
    )
}

/// Strict ASCII-digits `u32` parse: Rust's `parse` accepts a leading
/// `+`, which `fromisoformat` (and the Django `date_re`s) reject.
fn parse_digits(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// ISO weekday 1-7 → `chrono::Weekday` (day 0 or 8+ reject; day 0 must
/// not underflow the `- 1`).
fn iso_weekday(day: u32) -> Option<chrono::Weekday> {
    chrono::Weekday::try_from(u8::try_from(day).ok()?.checked_sub(1)?).ok()
}

/// The `fromisoformat` date half: `YYYY-MM-DD`, `YYYYMMDD`, `YYYY-Www[-D]`,
/// `YYYYWww[D]` (week without day → Monday). Ordinals are rejected
/// (verified live).
fn parse_iso_date_part(text: &str) -> Option<NaiveDate> {
    if text.len() >= 4 && text.as_bytes().get(4) == Some(&b'W') {
        // `YYYYWww[D]` basic week. Length + ASCII first: short inputs
        // (`"2024W"`) or multibyte bytes would panic the slices below,
        // while `fromisoformat` just rejects them.
        if !(text.len() == 7 || text.len() == 8) || !text.is_ascii() {
            return None;
        }
        let year = parse_digits(&text[0..4])? as i32;
        let week = parse_digits(&text[5..7])?;
        let weekday = if text.len() == 8 {
            parse_digits(&text[7..8])?
        } else {
            1
        };
        return NaiveDate::from_isoywd_opt(year, week, iso_weekday(weekday)?);
    }
    if text.contains('W') {
        // `YYYY-Www[-D]` extended week: exactly four digits, a dash, `W`
        // (short years like `24W05` reject).
        let mut parts = text.split('W');
        let year_text = parts.next()?;
        let year_digits = year_text.strip_suffix('-')?;
        if year_digits.len() != 4 {
            return None;
        }
        let year = parse_digits(year_digits)? as i32;
        let rest = parts.next()?;
        if parts.next().is_some() {
            return None;
        }
        // ASCII first: a multibyte `rest` of byte-length 3 would panic
        // the `[0..2]` slice below (`fromisoformat` rejects it instead).
        if !rest.is_ascii() {
            return None;
        }
        let (week_text, weekday) = match rest.split_once('-') {
            Some((week, day)) => (week, parse_digits(day)?),
            None if rest.len() == 2 => (rest, 1),
            None if rest.len() == 3 => (&rest[0..2], parse_digits(&rest[2..3])?),
            None => return None,
        };
        if week_text.len() != 2 {
            return None;
        }
        let week = parse_digits(week_text)?;
        return NaiveDate::from_isoywd_opt(year, week, iso_weekday(weekday)?);
    }
    if text.len() == 8 && text.bytes().all(|b| b.is_ascii_digit()) {
        // `YYYYMMDD` basic.
        return NaiveDate::from_ymd_opt(
            text[0..4].parse().ok()?,
            text[4..6].parse().ok()?,
            text[6..8].parse().ok()?,
        );
    }
    if text.len() == 10 {
        // `YYYY-MM-DD` extended (strictly zero-padded here; the 1-2-digit
        // shapes fall through to the `date_re`/`datetime_re` fallbacks).
        let bytes = text.as_bytes();
        if bytes[4] == b'-' && bytes[7] == b'-' {
            return NaiveDate::from_ymd_opt(
                parse_digits(&text[0..4])? as i32,
                parse_digits(&text[5..7])?,
                parse_digits(&text[8..10])?,
            );
        }
    }
    None
}

/// Django `parse_datetime` + DRF `enforce_timezone` in one: parses the
/// `fromisoformat` surface (verified live against the venv oracle), then
/// interprets naive values in the request user's zone (`make_aware`) and
/// converts aware values to the stored UTC instant. Errors map to the DRF
/// `invalid` / `overflow` / `make_aware` messages.
fn parse_drf_datetime(text: &str, timezone: &Tz) -> Result<DateTime<Utc>, Vec<String>> {
    let invalid = || vec![MSG_DATETIME_INVALID.to_owned()];
    // Trailing whitespace is tolerated only when no tz designator is
    // present (`'...05 '` ok, `'...+00:00 '` rejected, leading rejected).
    let mut candidates = vec![text];
    let trimmed_end = text.trim_end();
    if trimmed_end.len() != text.len() && split_tz_suffix(trimmed_end).1.is_none() {
        candidates.push(trimmed_end);
    }
    let mut parsed = None;
    for candidate in candidates {
        if let Some(value) = parse_iso_datetime(candidate) {
            parsed = Some(value);
            break;
        }
        if let Some(value) = parse_fallback_datetime(candidate) {
            parsed = Some(value);
            break;
        }
    }
    // DRF's `strptime(value, 'iso-8601')` fallthrough (`to_internal_value`
    // runs it when `parse_datetime` returns `None` — PIDASHCONV-773):
    // the literal matches case-insensitively and yields naive
    // 1900-01-01 through the zone-attach arm below. Checked on the RAW
    // text — Django's `strptime` sees the value before any strip, so a
    // padded literal still fails. Exact match (probed); ASCII-only (765
    // unicode-gap family).
    if parsed.is_none() && text.eq_ignore_ascii_case("iso-8601") {
        let naive = chrono::NaiveDate::from_ymd_opt(1900, 1, 1)
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .expect("1900-01-01 valid");
        parsed = Some((naive, None));
    }
    let (naive, offset_secs) = parsed.ok_or_else(invalid)?;
    match offset_secs {
        Some(offset) => {
            let aware = naive
                .and_utc()
                .checked_sub_signed(chrono::TimeDelta::seconds(offset))
                .ok_or_else(|| vec![MSG_DATETIME_OVERFLOW.to_owned()])?;
            Ok(aware)
        }
        None => match timezone.from_local_datetime(&naive) {
            chrono::MappedLocalTime::Single(aware) => Ok(aware.to_utc()),
            chrono::MappedLocalTime::Ambiguous(early, _) => Ok(early.to_utc()),
            chrono::MappedLocalTime::None => Err(vec![format!(
                "Invalid datetime for the timezone \"{timezone}\"."
            )]),
        },
    }
}

/// Split a trailing tz designator: `Z` (uppercase only), `±HH:MM[:SS[.f]]`,
/// `±HHMM`, `±HH`, with an optional single space before it. Returns the
/// head and the offset in seconds (fractional offset seconds truncate,
/// matching `timedelta`).
fn split_tz_suffix(text: &str) -> (&str, Option<i64>) {
    if let Some(head) = text.strip_suffix('Z') {
        if head.len() < text.len() && !head.is_empty() {
            return (head, Some(0));
        }
    }
    // Find the last `+`/`-` that starts the offset (not the date's dashes:
    // the offset sign sits after the time separator / at least past index
    // 10... simpler: scan from the end for the last sign preceded by a
    // digit or space, with only offset chars after it).
    let bytes = text.as_bytes();
    let mut sign_at = None;
    for (index, byte) in bytes.iter().enumerate().rev() {
        if *byte == b'+' || *byte == b'-' {
            let tail = &text[index + 1..];
            if !tail.is_empty()
                && tail
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b':' || b == b'.')
            {
                sign_at = Some(index);
                break;
            }
        }
        if *byte == b'T' || *byte == b't' || *byte == b' ' || *byte == b'W' {
            // A time/date separator left of any sign: the offset must
            // come after the time, so there is no tz suffix. Stop.
            break;
        }
    }
    let Some(at) = sign_at else {
        return (text, None);
    };
    // The sign must follow the time part (there is a time separator left
    // of it and at least `HH:MM`/`HHMM` after the separator).
    let head = text[..at].trim_end();
    let tail = text[at + 1..].trim_start();
    if tail.is_empty() {
        return (text, None);
    }
    let sign: i64 = if bytes[at] == b'-' { -1 } else { 1 };
    let digits: String = tail.chars().filter(|ch| ch.is_ascii_digit()).collect();
    // Colon offsets are strict: exactly-two-digit hours/minutes, and
    // seconds of exactly two digits plus an optional `.`/`,` fraction
    // (truncated — `+00:00:00.5` is `+00:00`).
    let offset = if tail.contains(':') {
        let mut parts = tail.split(':');
        let (Some(hours_text), Some(minutes_text)) = (parts.next(), parts.next()) else {
            return (text, None);
        };
        let seconds_text = parts.next();
        if parts.next().is_some() || hours_text.len() != 2 || minutes_text.len() != 2 {
            return (text, None);
        }
        let (Some(hours), Some(minutes)) = (parse_digits(hours_text), parse_digits(minutes_text))
        else {
            return (text, None);
        };
        let seconds: i64 = match seconds_text {
            None => 0,
            Some(sec_text) => {
                let (int, frac) = match sec_text.split_once(['.', ',']) {
                    None => (sec_text, None),
                    Some((int, frac)) => (int, Some(frac)),
                };
                if int.len() != 2 {
                    return (text, None);
                }
                let Some(seconds) = parse_digits(int) else {
                    return (text, None);
                };
                if frac.is_some_and(|f| f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit())) {
                    return (text, None);
                }
                seconds as i64
            }
        };
        hours as i64 * 3600 + minutes as i64 * 60 + seconds
    } else if digits.len() == tail.len() && (tail.len() == 2 || tail.len() == 4) {
        let hours: i64 = tail[0..2].parse().unwrap_or(-1);
        let minutes: i64 = if tail.len() == 4 {
            tail[2..4].parse().unwrap_or(-1)
        } else {
            0
        };
        if hours < 0 || minutes < 0 {
            return (text, None);
        }
        hours * 3600 + minutes * 60
    } else {
        return (text, None);
    };
    // `+99:99` parses numerically here; the range gate below rejects it
    // (offsets beyond ±23:59 raise in `fromisoformat`, surfacing as
    // `invalid` through DRF's suppress).
    if offset.abs() >= 24 * 3600 {
        return (head, Some(i64::MAX));
    }
    (head, Some(sign * offset))
}

/// The `fromisoformat` surface: extended/basic/week dates, any single-char
/// separator, `HH[:MM[:SS[.ffffff]]]` / basic `HHMM[SS]` times, comma-or-dot
/// fractions (truncated past 6 digits), optional tz. Returns the naive wall
/// time plus the tz offset in seconds when present.
fn parse_iso_datetime(text: &str) -> Option<(NaiveDateTime, Option<i64>)> {
    use chrono::NaiveDateTime;
    let (head, offset) = split_tz_suffix(text);
    if offset == Some(i64::MAX) {
        return None;
    }
    // Date-only (midnight) — but a trailing bare separator (`'...T'`) is
    // rejected, and week/basic shapes route through the date parser.
    if let Some(date) = parse_iso_date_part(head) {
        return Some((date.and_hms_opt(0, 0, 0)?, offset));
    }
    // Split date | separator | time at the first non-date char.
    let (date_text, sep, time_text) = split_date_time(head)?;
    let _ = sep;
    let date = parse_iso_date_part(date_text)?;
    let naive_time = parse_iso_time(time_text)?;
    Some((NaiveDateTime::new(date, naive_time), offset))
}

/// Split `head` into date/separator/time: the date part is the longest
/// leading run the date parser accepts... simpler: find the separator as
/// the first char class break after the date (any single char, 3.11+).
fn split_date_time(head: &str) -> Option<(&str, char, &str)> {
    // Candidate date lengths: `YYYY-MM-DD` (10), `YYYYMMDD` (8),
    // `YYYY-Www-D` (10), `YYYY-Www` (7), `YYYYWwwD` (8), `YYYYWww` (6).
    for len in [10usize, 8, 7, 6] {
        if head.len() <= len {
            continue;
        }
        // Checked split: a multibyte char straddling the cut is never a
        // valid date lead here — skip the length (shorter ones may still
        // match with a multibyte separator, which `fromisoformat`
        // accepts), it must not panic.
        let Some((date_text, rest)) = head.split_at_checked(len) else {
            continue;
        };
        if parse_iso_date_part(date_text).is_none() {
            continue;
        }
        let mut chars = rest.chars();
        let sep = chars.next()?;
        if sep.is_ascii_digit() {
            continue;
        }
        let time_text = &rest[sep.len_utf8()..];
        if time_text.is_empty() {
            continue;
        }
        return Some((date_text, sep, time_text));
    }
    None
}

/// `fromisoformat` time half: `HH[:MM[:SS[.ffffff]]]` (colons), `HHMM[SS]`
/// (basic), hour-only `HH`; comma-or-dot fractions truncate past 6 digits.
fn parse_iso_time(text: &str) -> Option<chrono::NaiveTime> {
    use chrono::NaiveTime;
    // Split off the fraction (`.` or `,` + 1+ digits; a bare trailing
    // separator is rejected).
    let (clock, micros) = match text.find(['.', ',']) {
        Some(dot) => {
            let (clock, frac) = text.split_at(dot);
            let digits = &frac[1..];
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let padded = format!("{digits:0<6}");
            let micros: u32 = padded[..6].parse().ok()?;
            (clock, micros)
        }
        None => (text, 0),
    };
    if clock.contains(':') {
        let mut parts = clock.split(':');
        let hour: u32 = parse_digits(parts.next()?)?;
        let minute: u32 = parts.next().map(parse_digits).unwrap_or(Some(0))?;
        let second: u32 = parts.next().map(parse_digits).unwrap_or(Some(0))?;
        if parts.next().is_some() {
            return None;
        }
        // `fromisoformat` requires zero-padded 2-digit fields here — the
        // 1-digit shapes fall through to the `datetime_re` fallback.
        for part in clock.split(':') {
            if part.len() != 2 {
                return None;
            }
        }
        NaiveTime::from_hms_micro_opt(hour, minute, second, micros)
    } else {
        // Basic `HH[MM[SS]]`: exactly 2/4/6 digits. ASCII first: a
        // multibyte clock of byte-length 4/6 would panic the slices
        // (`fromisoformat` rejects it instead).
        if !clock.is_ascii() {
            return None;
        }
        match clock.len() {
            2 => NaiveTime::from_hms_micro_opt(parse_digits(clock)?, 0, 0, micros),
            4 => NaiveTime::from_hms_micro_opt(
                parse_digits(&clock[0..2])?,
                parse_digits(&clock[2..4])?,
                0,
                micros,
            ),
            6 => NaiveTime::from_hms_micro_opt(
                parse_digits(&clock[0..2])?,
                parse_digits(&clock[2..4])?,
                parse_digits(&clock[4..6])?,
                micros,
            ),
            _ => None,
        }
    }
}

/// Django's `datetime_re` fallback: extended date with 1-2-digit fields,
/// `[T ]` separator, `H:M[:S[.ffffff]]` (fraction dot-or-comma, extras
/// ignored past 12 digits), optional `Z`/`±HH[[:]MM]` tz.
fn parse_fallback_datetime(text: &str) -> Option<(NaiveDateTime, Option<i64>)> {
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
    let (head, offset) = split_tz_suffix(text);
    if offset == Some(i64::MAX) {
        return None;
    }
    // Offset with seconds is `fromisoformat`-only; the fallback allows at
    // most `±HH[:]MM` — reject seconds here by re-checking the raw tail.
    if offset.is_some() {
        let tail = text[head.len()..].trim();
        let tail = tail.strip_prefix(['+', '-']).unwrap_or(tail);
        if tail.strip_suffix('Z').is_none() && tail.contains(':') && tail.matches(':').count() > 1 {
            return None;
        }
    }
    let sep_at = head.find(['T', ' '])?;
    let (date_text, time_text) = (&head[..sep_at], &head[sep_at + 1..]);
    let mut date_parts = date_text.split('-');
    let (year, month, day) = (date_parts.next()?, date_parts.next()?, date_parts.next()?);
    if date_parts.next().is_some() || year.len() != 4 {
        return None;
    }
    if !(1..=2).contains(&month.len()) || !(1..=2).contains(&day.len()) {
        return None;
    }
    let date = NaiveDate::from_ymd_opt(
        parse_digits(year)? as i32,
        parse_digits(month)?,
        parse_digits(day)?,
    )?;
    let (clock, micros) = match time_text.find(['.', ',']) {
        Some(dot) => {
            let (clock, frac) = time_text.split_at(dot);
            let digits = &frac[1..];
            if digits.is_empty() || digits.len() > 12 || !digits.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            let kept = &digits[..digits.len().min(6)];
            let padded = format!("{kept:0<6}");
            (clock, padded[..6].parse::<u32>().ok()?)
        }
        None => (time_text, 0),
    };
    let mut time_parts = clock.split(':');
    let (hour, minute) = (time_parts.next()?, time_parts.next()?);
    let second = time_parts.next().unwrap_or("0");
    if time_parts.next().is_some() {
        return None;
    }
    if !(1..=2).contains(&hour.len()) || !(1..=2).contains(&minute.len()) {
        return None;
    }
    if !(1..=2).contains(&second.len()) {
        return None;
    }
    let time = NaiveTime::from_hms_micro_opt(
        parse_digits(hour)?,
        parse_digits(minute)?,
        parse_digits(second)?,
        micros,
    )?;
    Some((NaiveDateTime::new(date, time), offset))
}

/// `PrimaryKeyRelatedField.to_internal_value` for UUID pks (labels,
/// assignees, `parent`, `state`, `project`, …): strings/ints/floats coerce
/// via `str()` (`%s`); bools REJECT; dicts/lists REJECT; UUID-shaped
/// strings (any case, braces, urn:, hex) parse. Returns the canonical
/// lowercase-hyphenated text for SQL binding.
/// The parsed-then-DB-checked shape of one pk input: DRF rejects bools
/// up front (`incorrect_type`); everything else goes to the queryset,
/// where Django's `UUIDField.to_python` tries `UUID(int=)` for ints and
/// `UUID(hex=)` otherwise — `AttributeError`/`ValueError` become the Django
/// `invalid` message echoing `str(value)`, and a parsed-but-missing id
/// becomes `does_not_exist` echoing the ORIGINAL spelling.
fn parse_uuid_pk(value: &Value) -> Result<Uuid, Vec<String>> {
    if value.is_boolean() {
        return Err(vec![format!(
            "Incorrect type. Expected pk value, received {0}.",
            json_compact_type(value)
        )]);
    }
    match value {
        // Ints take the `int=` path: any 128-bit magnitude parses (even
        // `5` → `00000000-...-000005`, then a `does_not_exist` lookup).
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            let magnitude: u128 = if let Some(int) = number.as_i64() {
                if int < 0 {
                    return Err(vec![format!("“{int}” is not a valid UUID.")]);
                }
                int as u128
            } else {
                // Guarded by `is_u64` above; every `u64` fits the `int=`
                // path (`u64::MAX < 2^128`).
                number.as_u64().unwrap_or(0) as u128
            };
            Ok(Uuid::from_u128(magnitude))
        }
        // Int-shaped literals past `u64` still take the `int=` path:
        // valid 128-bit magnitudes parse (then miss the existence
        // lookup); negatives, magnitudes past 2^128, and non-integers
        // are invalid.
        Value::Number(number) => {
            if let Some(literal) = int_literal(number) {
                if let Ok(magnitude) = literal.parse::<u128>() {
                    return Ok(Uuid::from_u128(magnitude));
                }
            }
            Err(vec![format!(
                "“{0}” is not a valid UUID.",
                uuid_echo(value)
            )])
        }
        Value::String(text) => match Uuid::parse_str(text) {
            Ok(uuid) => Ok(uuid),
            Err(_) => Err(vec![format!("“{text}” is not a valid UUID.")]),
        },
        // Floats, dicts, lists: the `hex=` path raises `AttributeError`
        // (`hex.replace` on a non-str), which Django reports with
        // `str(value)`.
        other => Err(vec![format!(
            "“{0}” is not a valid UUID.",
            uuid_echo(other)
        )]),
    }
}

/// `str(value)` for the UUID `invalid` echo: strings bare, ints exact,
/// floats via CPython `repr`, composites via CPython `repr`.
fn uuid_echo(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => json_number_str(number),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Null => "None".to_owned(),
        Value::Array(_) | Value::Object(_) => py_repr(value),
    }
}

/// `type(value).__name__` for the pk-type echo (`dict`/`list`/`bool`;
/// DRF renders the JSON type name, not `str`).
fn json_compact_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// `ListField.to_internal_value` for `label_ids`/`assignee_ids` (plain
/// JSON lists; HTML/querydict inputs cannot occur over JSON): non-lists
/// → `not_a_list`; each item through the UUID child (`allow_null` False →
/// `null` item errors); errors keyed by index.
/// `ListField` with a UUID child: non-lists fail `not_a_list` (a plain
/// message list under the field name — verified live); per-item failures
/// collect into an index-keyed dict (`run_child_validation`'s `errors`
/// dict, verified live as `{"0": [...]}`).
enum ListOutcome {
    /// `not_a_list` (or any whole-list failure): rendered as a list.
    Messages(Vec<String>),
    /// Parsed ids (empty when the input list was empty — `allow_empty`).
    Ids(Vec<Uuid>),
    /// Index-keyed item failures: rendered as a dict.
    Indexed(BTreeMap<usize, Vec<String>>),
}

fn parse_uuid_list(value: &Value) -> ListOutcome {
    let Value::Array(items) = value else {
        return ListOutcome::Messages(vec![format!(
            "Expected a list of items but got type \"{0}\".",
            json_type_name(value)
        )]);
    };
    let mut parsed = Vec::with_capacity(items.len());
    let mut errors = BTreeMap::new();
    for (index, item) in items.iter().enumerate() {
        if item.is_null() {
            errors.insert(index, vec!["This field may not be null.".to_owned()]);
            continue;
        }
        match parse_uuid_pk(item) {
            Ok(pk) => parsed.push(pk),
            Err(messages) => {
                errors.insert(index, messages);
            }
        }
    }
    if errors.is_empty() {
        ListOutcome::Ids(parsed)
    } else {
        ListOutcome::Indexed(errors)
    }
}

/// `type(value).__name__` (`NoneType`/`bool`/`int`/`float`/`str`/`list`/
/// `dict` — DRF renders the Python type name). Huge int literals classify
/// as `int` (CPython ints are unbounded — `json.loads` never produces a
/// float from one).
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) => {
            if int_literal(number).is_some() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// `to_internal_value`'s non-`Mapping` arm (`serializers.py:483-489`): a
/// non-dict body fails the WHOLE serializer as `non_field_errors`, before
/// any field runs (so no `required` errors join it).
fn non_dict_response(value: &Value) -> Response {
    let message = format!(
        "Invalid data. Expected a dictionary, but got {}.",
        json_type_name(value)
    );
    let detail = serde_json::to_string(&message).expect("non-dict detail");
    json_bad_request(&format!("{{\"non_field_errors\":[{detail}]}}"))
}

// ---------------------------------------------------------------------------
// Field-validation table (48 writable fields × 3 shapes) + driver
// ---------------------------------------------------------------------------

/// One writable field's validation shape. The driver walks the table in
/// `fields` order and renders errors under the field name, which reproduces
/// DRF's error-key order and message shapes exactly.
/// One writable field's validation shape, in `get_fields()` order (the
/// live serializer dump: explicit fields first, auto fields after — error
/// keys render in this order). Read-only fields (`id`, `project_id`,
/// `workspace_id`, `created_at`, `updated_at`, `created_by`, `updated_by`,
/// `project`, `workspace`, `description_binary`, `assignees`, `labels`) are
/// absent: their input is silently ignored.
struct FieldSpec {
    name: &'static str,
    shape: FieldShape,
    allow_null: bool,
}

/// The DRF field class behind each writable name, with the dumped bounds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FieldShape {
    /// `PrimaryKeyRelatedField` with the field's queryset for the
    /// `does_not_exist` leg.
    UuidPk(PkQueryset),
    /// `ListField(child=PrimaryKeyRelatedField)`.
    UuidList(PkQueryset),
    Char {
        max_length: Option<usize>,
        allow_blank: bool,
        branch_regex: bool,
    },
    Integer {
        min_value: i64,
        max_value: i64,
    },
    Float,
    Boolean,
    Date,
    DateTime,
    Json,
    Choice {
        options: &'static [&'static str],
        allow_blank: bool,
    },
}

/// The queryset behind each pk field's `.get(pk=)` (verified live,
/// including the deleted-row split: `state_id` uses the unscoped manager,
/// `state` the triage-excluding one).
#[derive(Clone, Copy, PartialEq, Eq)]
enum PkQueryset {
    /// `State.all_state_objects` (everything, even soft-deleted).
    StatesAll,
    /// `State.objects` (no triage group, no soft-deleted).
    States,
    /// `Issue.all_objects` (even soft-deleted).
    IssuesAll,
    /// `Issue.objects` (no soft-deleted).
    Issues,
    /// `Pod.all_objects`.
    PodsAll,
    /// `Label.objects` (no soft-deleted).
    Labels,
    /// `User.objects` (the users table has no soft-delete column).
    Users,
    /// `EstimatePoint.objects` (no soft-deleted).
    Estimates,
    /// `IssueType.objects` (no soft-deleted).
    Types,
}

/// The 29 writable fields of `IssueCreateSerializer` (create/PATCH/PUT all
/// share the class; PUT differs only in `partial=False` + context), in
/// `get_fields()` order, with the dumped flags. Verified live field by
/// field (null verdicts, bounds, choices, querysets).
const FIELD_TABLE: &[FieldSpec] = &[
    FieldSpec {
        name: "state_id",
        shape: FieldShape::UuidPk(PkQueryset::StatesAll),
        allow_null: true,
    },
    FieldSpec {
        name: "parent_id",
        shape: FieldShape::UuidPk(PkQueryset::IssuesAll),
        allow_null: true,
    },
    FieldSpec {
        name: "assigned_pod_id",
        shape: FieldShape::UuidPk(PkQueryset::PodsAll),
        allow_null: true,
    },
    FieldSpec {
        name: "label_ids",
        shape: FieldShape::UuidList(PkQueryset::Labels),
        allow_null: false,
    },
    FieldSpec {
        name: "assignee_ids",
        shape: FieldShape::UuidList(PkQueryset::Users),
        allow_null: false,
    },
    FieldSpec {
        name: "deleted_at",
        shape: FieldShape::DateTime,
        allow_null: true,
    },
    FieldSpec {
        name: "point",
        shape: FieldShape::Integer {
            min_value: 0,
            max_value: 12,
        },
        allow_null: true,
    },
    FieldSpec {
        name: "name",
        shape: FieldShape::Char {
            max_length: Some(255),
            allow_blank: false,
            branch_regex: false,
        },
        allow_null: false,
    },
    FieldSpec {
        name: "description_json",
        shape: FieldShape::Json,
        allow_null: false,
    },
    FieldSpec {
        name: "description_html",
        shape: FieldShape::Char {
            max_length: None,
            allow_blank: true,
            branch_regex: false,
        },
        allow_null: false,
    },
    FieldSpec {
        name: "description_stripped",
        shape: FieldShape::Char {
            max_length: None,
            allow_blank: true,
            branch_regex: false,
        },
        allow_null: true,
    },
    FieldSpec {
        name: "priority",
        shape: FieldShape::Choice {
            options: &["urgent", "high", "medium", "low", "none"],
            allow_blank: false,
        },
        allow_null: false,
    },
    FieldSpec {
        name: "complexity_score",
        shape: FieldShape::Integer {
            min_value: 0,
            max_value: 10,
        },
        allow_null: false,
    },
    FieldSpec {
        name: "start_date",
        shape: FieldShape::Date,
        allow_null: true,
    },
    FieldSpec {
        name: "target_date",
        shape: FieldShape::Date,
        allow_null: true,
    },
    FieldSpec {
        name: "sequence_id",
        shape: FieldShape::Integer {
            min_value: -2_147_483_648,
            max_value: 2_147_483_647,
        },
        allow_null: false,
    },
    FieldSpec {
        name: "sort_order",
        shape: FieldShape::Float,
        allow_null: false,
    },
    FieldSpec {
        name: "completed_at",
        shape: FieldShape::DateTime,
        allow_null: true,
    },
    FieldSpec {
        name: "archived_at",
        shape: FieldShape::Date,
        allow_null: true,
    },
    FieldSpec {
        name: "is_draft",
        shape: FieldShape::Boolean,
        allow_null: false,
    },
    FieldSpec {
        name: "external_source",
        shape: FieldShape::Char {
            max_length: Some(255),
            allow_blank: true,
            branch_regex: false,
        },
        allow_null: true,
    },
    FieldSpec {
        name: "external_id",
        shape: FieldShape::Char {
            max_length: Some(255),
            allow_blank: true,
            branch_regex: false,
        },
        allow_null: true,
    },
    FieldSpec {
        name: "git_work_branch",
        shape: FieldShape::Char {
            max_length: Some(128),
            allow_blank: true,
            branch_regex: true,
        },
        allow_null: false,
    },
    FieldSpec {
        name: "created_via",
        shape: FieldShape::Char {
            max_length: Some(32),
            allow_blank: true,
            branch_regex: false,
        },
        allow_null: true,
    },
    FieldSpec {
        name: "agent_executor",
        shape: FieldShape::Choice {
            options: &["local_runner", "cloud_agent", "managed_runner"],
            allow_blank: true,
        },
        allow_null: true,
    },
    FieldSpec {
        name: "parent",
        shape: FieldShape::UuidPk(PkQueryset::Issues),
        allow_null: true,
    },
    FieldSpec {
        name: "state",
        shape: FieldShape::UuidPk(PkQueryset::States),
        allow_null: true,
    },
    FieldSpec {
        name: "estimate_point",
        shape: FieldShape::UuidPk(PkQueryset::Estimates),
        allow_null: true,
    },
    FieldSpec {
        name: "type",
        shape: FieldShape::UuidPk(PkQueryset::Types),
        allow_null: true,
    },
];

/// A validated field value in its SQL-binding form.
#[derive(Debug, Clone)]
enum Validated {
    Null,
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Date(NaiveDate),
    DateTime(DateTime<Utc>),
    Json(Value),
    Uuid(Uuid),
    UuidList(Vec<Uuid>),
    /// `assigned_pod_id`: Django's field `get()` resolves the full row and
    /// `validate()` reuses the object — one query, not exists + fetch.
    Pod(PodRef),
    /// `state` / `state_id`: the field `get()` resolves the row and `save()`
    /// reads `.group` off the cached object — the group rides along so the
    /// save issues no extra query, like Django.
    State(Uuid, String),
}

/// Which request shape is validating: all three share the field table;
/// create and PUT are full updates (`name` required), PATCH is partial.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RequestShape {
    Create,
    Patch,
    Put,
}

impl RequestShape {
    fn partial(self) -> bool {
        self == RequestShape::Patch
    }
}

/// One field's shape failure: `Invalid` carries its 400 detail;
/// `Db` is a pk-probe database failure (Django's 500 — the database
/// raised mid-`is_valid()`; it must never render as a field error).
enum ShapeError {
    Invalid(Value),
    Db,
}

/// Field-pass failure: `Invalid` renders the 400 error object; `Db` maps
/// to the 500 body.
enum FieldPassFailure {
    Invalid(BTreeMap<String, Value>),
    Db,
}

/// Map a field-pass failure to its response.
fn field_pass_response(failure: FieldPassFailure) -> HandlerResult {
    match failure {
        FieldPassFailure::Invalid(errors) => Ok(json_bad_request(&render_field_errors(&errors))),
        FieldPassFailure::Db => Err(Denial::ServerError),
    }
}

/// `is_valid()`'s field pass: walk the table in order, `get_value` per
/// key (JSON + HTML-input rules), null verdicts, shape parsers, then the
/// pk existence legs. Unknown keys are silently ignored (DRF default).
/// Returns the validated map or the 400 error object (keys in table order,
/// then `non_field_errors` never — that's the `validate()` leg's).
async fn validate_issue_body(
    pool: &sqlx::PgPool,
    data: &RequestData,
    shape: RequestShape,
    timezone: &Tz,
) -> Result<BTreeMap<&'static str, Validated>, FieldPassFailure> {
    let mut validated: BTreeMap<&'static str, Validated> = BTreeMap::new();
    let mut errors: BTreeMap<String, Value> = BTreeMap::new();
    // NOTE: `BTreeMap` sorts error keys alphabetically, but DRF emits them
    // in table order — the caller re-sorts via `render_field_errors`.
    for spec in FIELD_TABLE {
        let required = !shape.partial() && spec.name == "name";
        match field_input(data, spec, shape.partial(), required) {
            FieldInput::Skip => {}
            FieldInput::Fail(messages) => {
                errors.insert(
                    spec.name.to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
            FieldInput::Value(raw) => {
                if raw.is_null() {
                    if spec.allow_null {
                        validated.insert(spec.name, Validated::Null);
                    } else {
                        errors.insert(
                            spec.name.to_owned(),
                            Value::Array(vec![Value::String(MSG_NULL.to_owned())]),
                        );
                    }
                    continue;
                }
                match validate_shape(pool, spec, raw, timezone).await {
                    Ok(value) => {
                        validated.insert(spec.name, value);
                    }
                    Err(ShapeError::Invalid(detail)) => {
                        errors.insert(spec.name.to_owned(), detail);
                    }
                    Err(ShapeError::Db) => return Err(FieldPassFailure::Db),
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(validated)
    } else {
        Err(FieldPassFailure::Invalid(errors))
    }
}

/// Render the field-error map in TABLE order (DRF's error-key order),
/// compact JSON.
fn render_field_errors(errors: &BTreeMap<String, Value>) -> String {
    let mut out = String::from("{");
    let mut first = true;
    for spec in FIELD_TABLE {
        let Some(detail) = errors.get(spec.name) else {
            continue;
        };
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(&serde_json::to_string(spec.name).expect("field name"));
        out.push(':');
        out.push_str(&serde_json::to_string(detail).expect("field detail"));
    }
    if let Some(detail) = errors.get("non_field_errors") {
        if !first {
            out.push(',');
        }
        out.push_str("\"non_field_errors\":");
        out.push_str(&serde_json::to_string(detail).expect("non-field detail"));
    }
    out.push('}');
    out
}

/// One field's `get_value`: JSON reads the key (`empty` when missing);
/// HTML-input applies the `''`/missing rules (`fields.py:415-427`) and
/// file objects (present keys) fall through to shape-`invalid`.
enum FieldInput<'a> {
    Skip,
    Fail(Vec<String>),
    Value(&'a Value),
}

/// Shared temporaries for synthesized HTML-input values.
const HTML_FALSE: Value = Value::Bool(false);
const HTML_NULL: Value = Value::Null;

fn field_input<'a>(
    data: &'a RequestData,
    spec: &FieldSpec,
    partial: bool,
    required: bool,
) -> FieldInput<'a> {
    // Uploads present under the key read as the file object, which every
    // shape below rejects — except the two `ListField`s, whose HTML
    // `getlist` the shared spec already materialized as the array.
    if let InputRef::Files = data.input(spec.name) {
        if !matches!(spec.shape, FieldShape::UuidList(_)) {
            return FieldInput::Fail(vec![shape_invalid_message(spec)]);
        }
    }
    match data.input(spec.name) {
        InputRef::Missing => {
            if data.is_html {
                if partial {
                    return FieldInput::Skip;
                }
                // `default_empty_html`: only `BooleanField` sets one.
                if matches!(spec.shape, FieldShape::Boolean) {
                    return FieldInput::Value(&HTML_FALSE);
                }
                return FieldInput::Skip;
            }
            if required {
                return FieldInput::Fail(vec![MSG_REQUIRED.to_owned()]);
            }
            FieldInput::Skip
        }
        InputRef::Json(raw) => {
            if data.is_html {
                if let Value::String(text) = raw {
                    if text.is_empty() {
                        if spec.allow_null {
                            let allow_blank = matches!(
                                spec.shape,
                                FieldShape::Char {
                                    allow_blank: true,
                                    ..
                                } | FieldShape::Choice {
                                    allow_blank: true,
                                    ..
                                }
                            );
                            if !allow_blank {
                                return FieldInput::Value(&HTML_NULL);
                            }
                        } else if !required {
                            let allow_blank = matches!(
                                spec.shape,
                                FieldShape::Char {
                                    allow_blank: true,
                                    ..
                                } | FieldShape::Choice {
                                    allow_blank: true,
                                    ..
                                }
                            );
                            if !allow_blank {
                                return FieldInput::Skip;
                            }
                        }
                    }
                }
            }
            FieldInput::Value(raw)
        }
        InputRef::Files => match data.value.as_object().and_then(|map| map.get(spec.name)) {
            // Both text and files under a list key: the spec's array.
            Some(raw) => FieldInput::Value(raw),
            None => FieldInput::Fail(vec![shape_invalid_message(spec)]),
        },
    }
}

/// The shape's `invalid` message for file objects (every shape rejects
/// them; lists reject the merged value as `not_a_list`-adjacent invalid).
fn shape_invalid_message(spec: &FieldSpec) -> String {
    match spec.shape {
        FieldShape::Char { .. } => MSG_INVALID_STR.to_owned(),
        FieldShape::Integer { .. } => MSG_INVALID_INT.to_owned(),
        FieldShape::Float => MSG_INVALID_FLOAT.to_owned(),
        FieldShape::Boolean => MSG_INVALID_BOOL.to_owned(),
        FieldShape::Date => MSG_DATE_INVALID.to_owned(),
        FieldShape::DateTime => MSG_DATETIME_INVALID.to_owned(),
        FieldShape::UuidPk(_) => "Incorrect type. Expected pk value, received file.".to_owned(),
        FieldShape::UuidList(_) => "Expected a list of items but got type \"file\".".to_owned(),
        FieldShape::Json => MSG_JSON_INVALID.to_owned(),
        FieldShape::Choice { .. } => "\"<file>\" is not a valid choice.".to_owned(),
    }
}

/// One present, non-null value through its shape parser (+ the pk
/// existence legs). Errors render as a message list, except indexed list
/// failures (a dict).
async fn validate_shape(
    pool: &sqlx::PgPool,
    spec: &FieldSpec,
    raw: &Value,
    timezone: &Tz,
) -> Result<Validated, ShapeError> {
    let failed = |messages: Vec<String>| {
        ShapeError::Invalid(Value::Array(
            messages.into_iter().map(Value::String).collect(),
        ))
    };
    match spec.shape {
        FieldShape::UuidPk(queryset) => {
            let parsed = parse_uuid_pk(raw).map_err(failed)?;
            // `assigned_pod_id` resolves the row here (Django's `get()`);
            // every other pk field probes existence and `validate()` runs
            // its own project-scoped `EXISTS` — two queries, like Django.
            if spec.name == "assigned_pod_id" {
                match pod_ref(pool, &parsed).await {
                    Ok(Some(pod)) => return Ok(Validated::Pod(pod)),
                    Ok(None) => {
                        return Err(failed(vec![format!(
                            "Invalid pk \"{0}\" - object does not exist.",
                            does_not_exist_echo(raw, &parsed)
                        )]));
                    }
                    Err(_) => return Err(ShapeError::Db),
                }
            }
            if spec.name == "state" || spec.name == "state_id" {
                match state_row(pool, queryset, &parsed).await {
                    Ok(Some((id, group))) => return Ok(Validated::State(id, group)),
                    Ok(None) => {
                        return Err(failed(vec![format!(
                            "Invalid pk \"{0}\" - object does not exist.",
                            does_not_exist_echo(raw, &parsed)
                        )]));
                    }
                    Err(_) => return Err(ShapeError::Db),
                }
            }
            match pk_exists(pool, queryset, &parsed).await {
                Ok(true) => Ok(Validated::Uuid(parsed)),
                Ok(false) => Err(failed(vec![format!(
                    "Invalid pk \"{0}\" - object does not exist.",
                    does_not_exist_echo(raw, &parsed)
                )])),
                Err(_) => Err(ShapeError::Db),
            }
        }
        FieldShape::UuidList(queryset) => match parse_uuid_list(raw) {
            ListOutcome::Messages(messages) => Err(failed(messages)),
            ListOutcome::Ids(ids) => {
                let mut checked = Vec::with_capacity(ids.len());
                let mut errors = BTreeMap::new();
                for (index, id) in ids.iter().enumerate() {
                    match pk_exists(pool, queryset, id).await {
                        Ok(true) => checked.push(*id),
                        Ok(false) => {
                            let echo = list_item_echo(raw, index);
                            errors.insert(
                                index,
                                vec![format!("Invalid pk \"{echo}\" - object does not exist.")],
                            );
                        }
                        Err(_) => {
                            return Err(ShapeError::Db);
                        }
                    }
                }
                if errors.is_empty() {
                    Ok(Validated::UuidList(checked))
                } else {
                    Err(ShapeError::Invalid(indexed_errors(errors)))
                }
            }
            ListOutcome::Indexed(indexed) => Err(ShapeError::Invalid(indexed_errors(indexed))),
        },
        FieldShape::Char {
            max_length,
            allow_blank,
            branch_regex,
        } => {
            // `CharField.run_validation`: blank (post-trim) short-circuits
            // before `to_internal_value`.
            if char_is_blank(raw) {
                if allow_blank {
                    return Ok(Validated::Text(String::new()));
                }
                return Err(failed(vec![MSG_BLANK.to_owned()]));
            }
            let text = parse_char(raw, max_length).map_err(failed)?;
            if branch_regex && !branch_name_valid(&text) {
                return Err(failed(vec![BRANCH_MESSAGE.to_owned()]));
            }
            Ok(Validated::Text(text))
        }
        FieldShape::Integer {
            min_value,
            max_value,
        } => {
            match parse_integer(raw).map_err(failed)? {
                IntegerValue::Int(int) => {
                    if int < min_value {
                        return Err(failed(vec![format!(
                            "Ensure this value is greater than or equal to {min_value}."
                        )]));
                    }
                    if int > max_value {
                        return Err(failed(vec![format!(
                            "Ensure this value is less than or equal to {max_value}."
                        )]));
                    }
                    // `validate_complexity_score` (unreachable over the
                    // wire — the field bounds fire first — but cheap).
                    if spec.name == "complexity_score" {
                        if let Err(error) =
                            pidash_services::app_issues::serializers_create::validate_complexity_score(
                                Some(int as i32),
                            )
                        {
                            if let Some(detail) = error.raised_detail() {
                                if let Some(message) =
                                    detail.get("complexity_score").and_then(|v| v.as_str())
                                {
                                    return Err(failed(vec![message.to_owned()]));
                                }
                            }
                        }
                    }
                    Ok(Validated::Int(int))
                }
                IntegerValue::TooBig => Err(failed(vec![format!(
                    "Ensure this value is less than or equal to {max_value}."
                )])),
                IntegerValue::TooSmall => Err(failed(vec![format!(
                    "Ensure this value is greater than or equal to {min_value}."
                )])),
            }
        }
        FieldShape::Float => parse_float(raw).map(Validated::Float).map_err(failed),
        FieldShape::Boolean => parse_boolean(raw).map(Validated::Bool).map_err(failed),
        FieldShape::Date => parse_drf_date_value(raw)
            .map(Validated::Date)
            .map_err(failed),
        FieldShape::DateTime => parse_drf_datetime_value(raw, timezone)
            .map(Validated::DateTime)
            .map_err(failed),
        FieldShape::Json => parse_json_value(raw).map(Validated::Json).map_err(failed),
        FieldShape::Choice {
            options,
            allow_blank,
        } => parse_choice(raw, options, allow_blank)
            .map(Validated::Text)
            .map_err(failed),
    }
}

/// Index-keyed list failures as a JSON dict.
fn indexed_errors(indexed: BTreeMap<usize, Vec<String>>) -> Value {
    Value::Object(
        indexed
            .into_iter()
            .map(|(index, messages)| {
                (
                    index.to_string(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                )
            })
            .collect(),
    )
}

/// `str(data).strip() == ''` over JSON scalars (the `CharField`
/// blank short-circuit; composites are never blank — they fail
/// `to_internal_value` instead).
fn char_is_blank(raw: &Value) -> bool {
    match raw {
        Value::String(text) => text.trim().is_empty(),
        Value::Number(number) => json_number_str(number).trim().is_empty(),
        Value::Bool(_) | Value::Null | Value::Array(_) | Value::Object(_) => false,
    }
}

/// The `git_work_branch` model `RegexValidator`
/// (`^[A-Za-z0-9._/-]*$`, verified live — DRF maps it, so `"!!!"` 400s).
fn branch_name_valid(text: &str) -> bool {
    text.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-'))
}

const BRANCH_MESSAGE: &str = "Branch name may contain only letters, numbers, and . _ / -";

/// The `does_not_exist` echo: `str()` of the ORIGINAL input spelling
/// (uppercase/braced UUIDs echo verbatim; ints echo as ints).
fn does_not_exist_echo(raw: &Value, parsed: &Uuid) -> String {
    match raw {
        Value::String(text) => text.clone(),
        Value::Number(number) => json_number_str(number),
        _ => parsed.hyphenated().to_string(),
    }
}

/// The original spelling of one list item (for its `does_not_exist`
/// echo): the raw array element re-stringified.
fn list_item_echo(raw: &Value, index: usize) -> String {
    match raw {
        Value::Array(items) => match items.get(index) {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Number(number)) => json_number_str(number),
            Some(other) => uuid_echo(other),
            None => String::new(),
        },
        _ => String::new(),
    }
}

/// `queryset.get(pk=)` existence for each pk field's manager.
async fn pk_exists(
    pool: &sqlx::PgPool,
    queryset: PkQueryset,
    id: &Uuid,
) -> Result<bool, sqlx::Error> {
    let sql = match queryset {
        PkQueryset::StatesAll => "SELECT 1 FROM states WHERE id = $1",
        PkQueryset::States => {
            "SELECT 1 FROM states WHERE id = $1 AND deleted_at IS NULL AND \"group\" != 'triage'"
        }
        PkQueryset::IssuesAll => "SELECT 1 FROM issues WHERE id = $1",
        PkQueryset::Issues => "SELECT 1 FROM issues WHERE id = $1 AND deleted_at IS NULL",
        PkQueryset::PodsAll => "SELECT 1 FROM pod WHERE id = $1",
        PkQueryset::Labels => "SELECT 1 FROM labels WHERE id = $1 AND deleted_at IS NULL",
        PkQueryset::Users => "SELECT 1 FROM users WHERE id = $1",
        PkQueryset::Estimates => {
            "SELECT 1 FROM estimate_points WHERE id = $1 AND deleted_at IS NULL"
        }
        PkQueryset::Types => "SELECT 1 FROM issue_types WHERE id = $1 AND deleted_at IS NULL",
    };
    let row: Option<(i32,)> = sqlx::query_as(sql).bind(id).fetch_optional(pool).await?;
    Ok(row.is_some())
}

/// `DateField.to_internal_value`: strings through `parse_date`; anything
/// else → `invalid`.
fn parse_drf_date_value(value: &Value) -> Result<NaiveDate, Vec<String>> {
    match value {
        Value::String(text) => {
            parse_drf_date(text).ok_or_else(|| vec![MSG_DATE_INVALID.to_owned()])
        }
        _ => Err(vec![MSG_DATE_INVALID.to_owned()]),
    }
}

/// `DateTimeField.to_internal_value`: strings through `parse_datetime` +
/// `enforce_timezone`; anything else → `invalid`.
fn parse_drf_datetime_value(value: &Value, timezone: &Tz) -> Result<DateTime<Utc>, Vec<String>> {
    match value {
        Value::String(text) => parse_drf_datetime(text, timezone),
        _ => Err(vec![MSG_DATETIME_INVALID.to_owned()]),
    }
}

// ---------------------------------------------------------------------------
// `validate()` probes: one query each, issued in Django's order
// ---------------------------------------------------------------------------

/// Django renders `filter(project_id=None, ...)` as `IS NULL` — the PUT
/// context gap reaches these arms (no services const covers them; the
/// `IS NULL` verdicts produce the live-verified `non_field` 400s).
const STATE_EXISTS_NULL_PROJECT_SQL: &str =
    "SELECT 1 FROM states WHERE project_id IS NULL AND id = $1 \
     AND deleted_at IS NULL AND NOT (\"group\" = 'triage') LIMIT 1";
const PARENT_EXISTS_NULL_PROJECT_SQL: &str =
    "SELECT 1 FROM issues WHERE project_id IS NULL AND id = $1 AND deleted_at IS NULL LIMIT 1";
const ESTIMATE_EXISTS_NULL_PROJECT_SQL: &str =
    "SELECT 1 FROM estimate_points WHERE project_id IS NULL AND id = $1 AND deleted_at IS NULL LIMIT 1";

/// `UserLLMConfig.objects.filter(user=user).first()` for the
/// `managed_llm_profile` seam (`model_provider.py:67-68`, `:85-91`): no
/// model ordering, so unordered `LIMIT 1`; the verdict is
/// `bool(api_key_encrypted)`.
const ASSISTANT_KEY_SQL: &str =
    "SELECT api_key_encrypted FROM assistant_user_llm_config WHERE user_id = $1 LIMIT 1";

/// 201 with a pre-rendered JSON body (mirrors the sibling handlers).
fn json_created(body: String) -> Response {
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("created response")
}

/// 400 with a pre-rendered JSON body (mirrors the sibling handlers).
fn json_bad_request(body: &str) -> Response {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_owned()))
        .expect("bad-request response")
}

/// Whether a `sqlx` failure is an integrity-constraint violation (SQLSTATE
/// class 23 — what Django surfaces as `IntegrityError` and the m2m legs
/// swallow).
fn is_integrity_violation(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db) => db.code().is_some_and(|code| code.starts_with("23")),
        _ => false,
    }
}

/// `Pod.all_objects.get(pk)` ([`POD_FETCH_SQL`]): tombstones included so
/// `validate()` can return the friendly deleted message.
async fn pod_ref(pool: &sqlx::PgPool, id: &Uuid) -> Result<Option<PodRef>, sqlx::Error> {
    let row: Option<(Uuid, Uuid, Option<DateTime<Utc>>)> = sqlx::query_as(POD_FETCH_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(id, project_id, deleted_at)| PodRef {
        id,
        project_id,
        deleted: deleted_at.is_some(),
    }))
}

async fn git_issue_synced(pool: &sqlx::PgPool, issue_id: &Uuid) -> Result<bool, sqlx::Error> {
    let row: Option<(i32,)> = sqlx::query_as(GIT_ISSUE_SYNC_PROBE_SQL)
        .bind(issue_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

async fn github_issue_synced(pool: &sqlx::PgPool, issue_id: &Uuid) -> Result<bool, sqlx::Error> {
    let row: Option<(i32,)> = sqlx::query_as(GITHUB_ISSUE_SYNC_PROBE_SQL)
        .bind(issue_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

async fn has_active_run(pool: &sqlx::PgPool, issue_id: &Uuid) -> Result<bool, sqlx::Error> {
    let row: Option<(i32,)> = sqlx::query_as(HAS_ACTIVE_RUN_SQL)
        .bind(issue_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

async fn filter_assignee_ids(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, sqlx::Error> {
    let sql = assignee_member_filter_sql(ids.len());
    let mut query = sqlx::query_as::<_, (Uuid,)>(&sql).bind(project_id);
    for id in ids {
        query = query.bind(id);
    }
    let rows: Vec<(Uuid,)> = query.fetch_all(pool).await?;
    Ok(rows.into_iter().map(|row| row.0).collect())
}

async fn filter_label_ids(
    pool: &sqlx::PgPool,
    project_id: Option<&Uuid>,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, sqlx::Error> {
    let sql = match project_id {
        Some(_) => label_filter_sql(ids.len()),
        None => label_filter_null_project_sql(ids.len()),
    };
    let mut query = sqlx::query_as::<_, (Uuid,)>(&sql);
    if let Some(project) = project_id {
        query = query.bind(project);
    }
    for id in ids {
        query = query.bind(id);
    }
    let rows: Vec<(Uuid,)> = query.fetch_all(pool).await?;
    Ok(rows.into_iter().map(|row| row.0).collect())
}

/// The step 9-11 project-scoped `EXISTS`: the `IS NULL` rendering when the
/// context project is `None` (PUT).
async fn project_scoped_exists(
    pool: &sqlx::PgPool,
    some_sql: &str,
    none_sql: &str,
    project_id: Option<&Uuid>,
    id: &Uuid,
) -> Result<bool, sqlx::Error> {
    let row: Option<(i32,)> = match project_id {
        Some(project) => {
            sqlx::query_as(some_sql)
                .bind(project)
                .bind(id)
                .fetch_optional(pool)
                .await?
        }
        None => {
            sqlx::query_as(none_sql)
                .bind(id)
                .fetch_optional(pool)
                .await?
        }
    };
    Ok(row.is_some())
}

async fn assistant_has_api_key(pool: &sqlx::PgPool, user_id: &Uuid) -> Result<bool, sqlx::Error> {
    let row: Option<(Option<Vec<u8>>,)> = sqlx::query_as(ASSISTANT_KEY_SQL)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.and_then(|row| row.0).is_some_and(|key| !key.is_empty()))
}

async fn fetch_project_ws(
    pool: &sqlx::PgPool,
    id: &Uuid,
) -> Result<Option<ResolvedProject>, sqlx::Error> {
    let row: Option<(Uuid, Uuid)> = sqlx::query_as(PROJECT_FETCH_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(project_id, workspace_id)| ResolvedProject {
        project_id,
        workspace_id,
    }))
}

async fn enrolled_runner(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
) -> Result<bool, sqlx::Error> {
    let row: Option<(i32,)> = sqlx::query_as(ENROLLED_MANAGED_RUNNERS_EXISTS_SQL)
        .bind(user_id)
        .bind(project_id)
        .bind(workspace_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

async fn online_runner(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
) -> Result<bool, sqlx::Error> {
    let bound = Utc::now() - chrono::Duration::seconds(HEARTBEAT_GRACE_SECS);
    let row: Option<(Uuid,)> = sqlx::query_as(ONLINE_MANAGED_RUNNER_SQL)
        .bind(user_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(bound)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

// ---------------------------------------------------------------------------
// `validate()` driver: attrs + probes + `issue_create_validate`
// ---------------------------------------------------------------------------

/// The looked-up issue row a write path validates against (update paths;
/// create passes `None`). Carries the `validate()`-consumed columns.
struct WriteInstance {
    id: Uuid,
    project_id: Uuid,
    workspace_id: Uuid,
    external_source: Option<String>,
    agent_executor: Option<String>,
    assigned_pod_id: Option<Uuid>,
    name: String,
    description_html: String,
    description_json: Value,
    description_stripped: Option<String>,
}

impl WriteInstance {
    /// The managed arm's project without a query: `issue.workspace_id`
    /// denormalizes `project.workspace_id`, so this is value-identical to
    /// the `instance.project` FK fetch Django issues there.
    fn resolved_project(&self) -> ResolvedProject {
        ResolvedProject {
            project_id: self.project_id,
            workspace_id: self.workspace_id,
        }
    }
}

/// Run the serializer `validate()` leg: build [`ValidateAttrs`] from the
/// field pass (dual sources resolved — the auto field wins when present),
/// issue exactly the reachable probes in Django's order, and map the
/// error. `project_id` is `None` on PUT — the context gap behind the
/// assignee `KeyError` 400, the silent label wipe, and the state/parent /
/// estimate `non_field` 400s (all verified live).
#[allow(clippy::result_large_err)]
async fn run_validate(
    pool: &sqlx::PgPool,
    state: &AppState,
    validated: &BTreeMap<&'static str, Validated>,
    project_id: Option<Uuid>,
    instance: Option<&WriteInstance>,
    actor: &CoreTenant,
) -> Result<ValidatedAttrs, Response> {
    let db_error = || Denial::ServerError.into_response();
    let text = |key: &str| match validated.get(key) {
        Some(Validated::Text(text)) => Some(text.as_str()),
        _ => None,
    };
    let uuid = |key: &str| match validated.get(key) {
        Some(Validated::Uuid(id)) => Some(*id),
        Some(Validated::State(id, _)) => Some(*id),
        _ => None,
    };
    // Dual sources: the auto field (`state`/`parent`, declared later)
    // overwrites its explicit twin's `attrs` slot whenever the KEY is
    // present — even when null (DRF loops fields in order, `set_value`
    // overwrites). Value-fallback would wrongly resurrect the twin.
    let state_value = if validated.contains_key("state") {
        uuid("state")
    } else {
        uuid("state_id")
    };
    let parent_value = if validated.contains_key("parent") {
        uuid("parent")
    } else {
        uuid("parent_id")
    };
    let state_touched = state_value.is_some();
    let parent_touched = parent_value.is_some();
    let assignee_ids: Option<Vec<Uuid>> = match validated.get("assignee_ids") {
        Some(Validated::UuidList(ids)) => Some(ids.clone()),
        _ => None,
    };
    let label_ids: Option<Vec<Uuid>> = match validated.get("label_ids") {
        Some(Validated::UuidList(ids)) => Some(ids.clone()),
        _ => None,
    };
    let assigned_pod: Option<Option<PodRef>> = match validated.get("assigned_pod_id") {
        None => None,
        Some(Validated::Null) => Some(None),
        Some(Validated::Pod(pod)) => Some(Some(*pod)),
        _ => None,
    };
    let agent_executor: Option<Option<&str>> = match validated.get("agent_executor") {
        None => None,
        Some(Validated::Null) => Some(None),
        Some(Validated::Text(executor)) => Some(Some(executor.as_str())),
        _ => None,
    };
    let description_stripped: Option<Option<&str>> = match validated.get("description_stripped") {
        None => None,
        Some(Validated::Null) => Some(None),
        Some(Validated::Text(stripped)) => Some(Some(stripped.as_str())),
        _ => None,
    };
    let description_json: Option<&Value> = match validated.get("description_json") {
        Some(Validated::Json(value)) => Some(value),
        _ => None,
    };
    let attrs = ValidateAttrs {
        locked: LockedIssueAttrs {
            name: text("name"),
            // RAW pre-sanitize input — the lock runs at step 1, sanitize
            // at step 5.
            description_html: text("description_html"),
            description_json,
            description_stripped,
            description_binary_present: false,
        },
        start_date: validated.get("start_date").and_then(|value| match value {
            Validated::Date(date) => Some(*date),
            _ => None,
        }),
        target_date: validated.get("target_date").and_then(|value| match value {
            Validated::Date(date) => Some(*date),
            _ => None,
        }),
        assigned_pod,
        agent_executor,
        description_html: text("description_html"),
        description_binary: None,
        assignee_ids: assignee_ids.as_deref(),
        label_ids: label_ids.as_deref(),
        state: state_value,
        parent: parent_value,
        estimate_point: uuid("estimate_point"),
        attrs_project: None,
    };
    let current = instance.map(|row| ValidateInstance {
        external_source: row.external_source.as_deref(),
        // The detail queryset carries no `is_synced` annotation
        // (`get_queryset` filters + `distinct()` only), so the probes run.
        annotated_is_synced: None,
        locked: LockedIssueValues {
            name: row.name.as_str(),
            description_html: row.description_html.as_str(),
            description_json: &row.description_json,
            description_stripped: row.description_stripped.as_deref(),
        },
        project_id: row.project_id,
        project: row.resolved_project(),
        assigned_pod_id: row.assigned_pod_id,
        agent_executor: row.agent_executor.as_deref(),
    });

    // Step 1 probes. `issue_is_actively_synced` short-circuits: empty /
    // missing source runs nothing; otherwise git, then github iff git
    // misses — ahead of the blocked-keys check either way.
    let synced_source = current
        .as_ref()
        .and_then(|row| row.external_source)
        .is_some_and(|source| !source.is_empty());
    // `synced_source` implies `instance` (the source comes from the row).
    let instance_id = instance.map(|row| row.id).unwrap_or(Uuid::nil());
    let git_sync = if synced_source {
        git_issue_synced(pool, &instance_id)
            .await
            .map_err(|_| db_error())?
    } else {
        false
    };
    let github_sync = if synced_source && !git_sync {
        github_issue_synced(pool, &instance_id)
            .await
            .map_err(|_| db_error())?
    } else {
        false
    };

    // Step 3 reachability (pure): the different-project / deleted legs
    // return before the reassign leg's `has_active_run` probe.
    let pod_pure_fail = match assigned_pod {
        Some(Some(pod)) => {
            let expected = project_id.or_else(|| instance.map(|row| row.project_id));
            expected.is_some_and(|expected| pod.project_id != expected) || pod.deleted
        }
        _ => false,
    };
    let pod_reassign_reached = !pod_pure_fail
        && assigned_pod.is_some()
        && instance.is_some_and(|row| row.assigned_pod_id.is_some())
        && assigned_pod.map(|pod| pod.map(|resolved| resolved.id))
            != Some(instance.and_then(|row| row.assigned_pod_id));
    let run_probe_one = if pod_reassign_reached {
        Some(
            has_active_run(pool, &instance_id)
                .await
                .map_err(|_| db_error())?,
        )
    } else {
        None
    };
    let pod_returns = pod_pure_fail || run_probe_one == Some(true);

    // Step 4 probes (unreached when step 3 returns). The pass-through
    // predicates mirror the kernel arm-for-arm so issuance matches.
    let mut fetched_project: Option<ResolvedProject> = None;
    let mut llm_profile = LlmProfile {
        available: false,
        reason_code: ManagedRunnerReason::LLM_CONFIG_MISSING.to_owned(),
    };
    let mut enrolled_exists = false;
    let mut online_exists = false;
    let mut executor_returns = false;
    let viewer = UserFlags {
        is_active: actor.user_active,
        is_bot: actor.user_bot,
    };
    if !pod_returns {
        if let Some(Some(executor)) = agent_executor {
            match AgentExecutorKind::from_value(executor) {
                None => executor_returns = true,
                Some(kind) => {
                    if kind == AgentExecutorKind::CloudAgent
                        && !cloud_agent_is_configured(&state.settings().cloud_agent)
                    {
                        executor_returns = true;
                    } else if kind == AgentExecutorKind::ManagedRunner {
                        let project = match instance {
                            Some(row) => Some(row.resolved_project()),
                            None => match project_id {
                                Some(id) => {
                                    let fetched = fetch_project_ws(pool, &id)
                                        .await
                                        .map_err(|_| db_error())?;
                                    fetched_project = fetched;
                                    fetched
                                }
                                None => None,
                            },
                        };
                        match project {
                            None => executor_returns = true,
                            Some(project) => {
                                // `managed_runner_availability` gates in
                                // fixed order: the LLM seam runs only
                                // past the instance switch + viewer
                                // legs; enrolled/online only past a
                                // usable profile (never in CE — the
                                // profile is always unavailable — but
                                // the structure mirrors the policy).
                                if managed_runner_is_enabled(&state.settings().managed_runner)
                                    && viewer.is_active
                                    && !viewer.is_bot
                                {
                                    let keyed = assistant_has_api_key(pool, &actor.user_id)
                                        .await
                                        .map_err(|_| db_error())?;
                                    llm_profile = LlmProfile {
                                        available: false,
                                        reason_code: if keyed {
                                            ManagedRunnerReason::BYOK_UNSUPPORTED.to_owned()
                                        } else {
                                            ManagedRunnerReason::LLM_CONFIG_MISSING.to_owned()
                                        },
                                    };
                                    if llm_profile.available {
                                        enrolled_exists = enrolled_runner(
                                            pool,
                                            &actor.user_id,
                                            &project.project_id,
                                            &project.workspace_id,
                                        )
                                        .await
                                        .map_err(|_| db_error())?;
                                        if enrolled_exists {
                                            online_exists = online_runner(
                                                pool,
                                                &actor.user_id,
                                                &project.project_id,
                                                &project.workspace_id,
                                            )
                                            .await
                                            .map_err(|_| db_error())?;
                                        }
                                    }
                                }
                                let verdict = managed_runner_availability(
                                    &state.settings().managed_runner,
                                    Some(&viewer),
                                    || llm_profile.clone(),
                                    enrolled_exists,
                                    online_exists,
                                );
                                if !verdict.available
                                    && verdict.reason_code
                                        != ManagedRunnerReason::NO_RUNNER_FOR_PROJECT
                                {
                                    executor_returns = true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // The executor mid-flight `has_active_run` probe: Django evaluates the
    // property per access, so pod-reassign + mid-flight issue the SAME
    // query TWICE when both arms are reached.
    let midflight_reached = !pod_returns
        && agent_executor.is_some()
        && instance.is_some_and(|row| Some(row.agent_executor.as_deref()) != agent_executor)
        && !executor_returns;
    let run_probe_two = if midflight_reached {
        Some(
            has_active_run(pool, &instance_id)
                .await
                .map_err(|_| db_error())?,
        )
    } else {
        None
    };
    let run_value = run_probe_one.or(run_probe_two).unwrap_or(false);

    // Steps 7-8. The assignee arm `KeyError`s (no probe) when the context
    // project is `None`; empty lists skip both filters.
    let filtered_assignees = match assignee_ids.as_deref() {
        Some(ids) if !ids.is_empty() && project_id.is_some() => Some(
            filter_assignee_ids(pool, &project_id.unwrap_or(Uuid::nil()), ids)
                .await
                .map_err(|_| db_error())?,
        ),
        _ => None,
    };
    let filtered_labels = match label_ids.as_deref() {
        Some(ids) if !ids.is_empty() => Some(
            filter_label_ids(pool, project_id.as_ref(), ids)
                .await
                .map_err(|_| db_error())?,
        ),
        _ => None,
    };
    // Steps 9-11 (touched + non-null only — a null clears without a probe).
    let state_ok = if state_touched {
        Some(
            project_scoped_exists(
                pool,
                STATE_EXISTS_SQL,
                STATE_EXISTS_NULL_PROJECT_SQL,
                project_id.as_ref(),
                &state_value.unwrap_or(Uuid::nil()),
            )
            .await
            .map_err(|_| db_error())?,
        )
    } else {
        None
    };
    let parent_ok = if parent_touched {
        Some(
            project_scoped_exists(
                pool,
                PARENT_EXISTS_SQL,
                PARENT_EXISTS_NULL_PROJECT_SQL,
                project_id.as_ref(),
                &parent_value.unwrap_or(Uuid::nil()),
            )
            .await
            .map_err(|_| db_error())?,
        )
    } else {
        None
    };
    let estimate_ok = match attrs.estimate_point {
        Some(id) => Some(
            project_scoped_exists(
                pool,
                ESTIMATE_EXISTS_SQL,
                ESTIMATE_EXISTS_NULL_PROJECT_SQL,
                project_id.as_ref(),
                &id,
            )
            .await
            .map_err(|_| db_error())?,
        ),
        None => None,
    };
    let policy = ExecutorPolicy {
        cloud: &state.settings().cloud_agent,
        managed: &state.settings().managed_runner,
        viewer: Some(&viewer),
    };
    let probes = ValidateProbes {
        git_sync_exists: &|| git_sync,
        github_sync_exists: &|| github_sync,
        has_active_run: &|| run_value,
        filter_assignees: &|_| filtered_assignees.clone().unwrap_or_default(),
        filter_labels: &|_| filtered_labels.clone().unwrap_or_default(),
        state_exists: &|| state_ok.unwrap_or(false),
        triage_state_exists: &|| false,
        parent_exists: &|| parent_ok.unwrap_or(false),
        estimate_exists: &|| estimate_ok.unwrap_or(false),
        fetch_project: &|| fetched_project,
        llm_profile: &|| llm_profile.clone(),
        enrolled_exists: &|| enrolled_exists,
        online_exists: &|| online_exists,
    };
    let ctx = ValidateContext {
        project_id,
        // No view path sets `allow_triage_state` — the triage manager leg
        // is unreachable and the probe stays unissued.
        allow_triage_state: false,
    };
    match issue_create_validate(&attrs, &ctx, current.as_ref(), &policy, &probes) {
        Ok(validated_attrs) => Ok(validated_attrs),
        Err(error) => Err(validate_error_response(&error)),
    }
}

/// Map a `validate()` failure to its wire response. `ContextMissing` is
/// the PUT assignee `KeyError`, which DRF's `handle_exception` renders as
/// 400 `{"error": ...}` — verified live, not a 500. A bare-string/list
/// `ValidationError` detail from `validate()` renders under
/// `non_field_errors` (DRF's `as_serializer_error`); a dict renders as-is.
fn validate_error_response(error: &CreateValidateError) -> Response {
    match error {
        CreateValidateError::ContextMissing { .. } => json_bad_request(KEY_ERROR_BODY),
        _ => match error.raised_detail() {
            Some(detail) => {
                let wrapped = match detail {
                    Value::Object(map) => {
                        let listed = map
                            .into_iter()
                            .map(|(key, value)| match value {
                                Value::Array(_) => (key, value),
                                single => (key, Value::Array(vec![single])),
                            })
                            .collect();
                        Value::Object(listed)
                    }
                    Value::Array(items) => {
                        let mut map = Map::new();
                        map.insert("non_field_errors".to_owned(), Value::Array(items));
                        Value::Object(map)
                    }
                    single => {
                        let mut map = Map::new();
                        map.insert("non_field_errors".to_owned(), Value::Array(vec![single]));
                        Value::Object(map)
                    }
                };
                let body = serde_json::to_string(&wrapped).unwrap_or_else(|_| "{}".to_owned());
                json_bad_request(&body)
            }
            None => Denial::ServerError.into_response(),
        },
    }
}

// ---------------------------------------------------------------------------
// Orchestration signals (explicit): `capture_prior_state` + `fire_state_transition`
// ---------------------------------------------------------------------------

/// Whether the request carries `X-Pi-Dash-Skip-Immediate-Dispatch: 1`
/// (Comment & Run owns dispatch — the transition must not fire its own
/// immediate dispatch). Only `partial_update` reads the header; create /
/// PUT / destroy always dispatch immediately.
fn skip_immediate_dispatch(headers: &HeaderMap) -> bool {
    headers
        .get(SKIP_DISPATCH_HEADER)
        .is_some_and(|value| value.as_bytes() == b"1")
}

/// The post-save fire: a non-transition answers `NoTransition` without
/// I/O; a failed handler is logged with the verbatim line (the counter
/// already bumped inside) and the save stands; only a *lookup* storage
/// failure propagates to the 500 — exactly the `try` placement in
/// `fire_state_transition`.
async fn fire_after_save(
    pool: &sqlx::PgPool,
    issue_id: Uuid,
    prev_state_id: Option<Uuid>,
    current_state_id: Option<Uuid>,
    dispatch_immediate: bool,
    created_by: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    let mut seam = CoreSignalSeam { pool };
    let mut preflight = CorePreflight;
    let outcome = fire_state_transition(
        &mut seam,
        &mut preflight,
        &FireRequest {
            issue_id,
            prev_state_id,
            current_state_id,
            dispatch_immediate,
            moved_by_run: None,
            now,
            jitter_secs: 0.0,
            created_by,
        },
    )
    .await
    .map_err(|_| Denial::ServerError)?;
    if let FireOutcome::Failed { log_line, .. } = outcome {
        tracing::error!("{log_line}");
    }
    Ok(())
}

/// The live 594 seam for the core saves, mirroring 656's
/// `ArchiveSignalSeam`. Only two methods run live:
/// [`EntriesSeam::prior_state_id`] (the pre-save snapshot) and
/// [`CreationSeam::state`] (the transition's state lookups) — the fire
/// short-circuits on equal states before any other seam use, and
/// transitions into non-trigger states return before the drivers run. A
/// transition that WOULD dispatch (into/out of a ticking state) answers a
/// store error, which the fire swallows like a handler raise (counter +
/// log line, save stands) rather than crashing the request — the same
/// truncation 656 ships; the HTTP surface is unaffected either way.
struct CoreSignalSeam<'a> {
    pool: &'a sqlx::PgPool,
}

/// The core saves never dispatch through preflight, so it never runs.
struct CorePreflight;

fn seam_unreachable<T>(method: &str) -> Result<T, CreationError> {
    Err(CreationError::Db(format!(
        "core signal seam: {method} runs only past a state change"
    )))
}

impl CreationSeam for CoreSignalSeam<'_> {
    async fn issue(&mut self, _issue_id: Uuid) -> Result<IssueView, CreationError> {
        seam_unreachable("issue")
    }

    async fn project(&mut self, _project_id: Uuid) -> Result<ProjectView, CreationError> {
        seam_unreachable("project")
    }

    async fn state(&mut self, state_id: Option<Uuid>) -> Result<Option<StateView>, CreationError> {
        let Some(state_id) = state_id else {
            return Ok(None);
        };
        let row: Option<(Uuid, String, String)> = sqlx::query_as(STATE_SELECT_SQL)
            .bind(state_id)
            .fetch_optional(self.pool)
            .await
            .map_err(|error| CreationError::Db(error.to_string()))?;
        match row {
            None => Err(CreationError::MissingRow(format!(
                "states row {state_id} is gone"
            ))),
            Some((id, name, group)) => Ok(Some(StateView { id, name, group })),
        }
    }

    async fn latest_prior_run(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        seam_unreachable("latest_prior_run")
    }

    async fn active_run_for(&mut self, _issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        seam_unreachable("active_run_for")
    }

    async fn run(&mut self, _run_id: Uuid) -> Result<Option<RunView>, CreationError> {
        seam_unreachable("run")
    }

    async fn runner(&mut self, _runner_id: Uuid) -> Result<Option<RunnerView>, CreationError> {
        seam_unreachable("runner")
    }

    async fn assigned_pod(&mut self, _pod_id: Uuid) -> Result<Option<PodView>, CreationError> {
        seam_unreachable("assigned_pod")
    }

    async fn default_pod_for_project(
        &mut self,
        _project_id: Uuid,
    ) -> Result<Option<PodView>, CreationError> {
        seam_unreachable("default_pod_for_project")
    }

    async fn resume_parent_run_id(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<Uuid>, CreationError> {
        seam_unreachable("resume_parent_run_id")
    }

    async fn work_item_id_for_run(&mut self, _run_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        seam_unreachable("work_item_id_for_run")
    }

    async fn lock_issue_for_handoff(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<LockedIssue>, CreationError> {
        seam_unreachable("lock_issue_for_handoff")
    }

    async fn lock_run_for_handoff(
        &mut self,
        _run_id: Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        seam_unreachable("lock_run_for_handoff")
    }

    async fn user_flags(&mut self, _user_id: Uuid) -> Result<UserFlags, CreationError> {
        seam_unreachable("user_flags")
    }

    async fn insert_run(&mut self, _row: &NewAgentRun) -> Result<RunView, CreationError> {
        seam_unreachable("insert_run")
    }

    async fn save_prompt(
        &mut self,
        _run_id: Uuid,
        _prompt: &str,
        _manifest: &Value,
    ) -> Result<(), CreationError> {
        seam_unreachable("save_prompt")
    }

    async fn save_run_config(
        &mut self,
        _run_id: Uuid,
        _config: &Value,
    ) -> Result<(), CreationError> {
        seam_unreachable("save_run_config")
    }

    async fn execution_fields(
        &mut self,
        _req: &ExecutionRequest,
    ) -> Result<ExecutionFields, ExecutionError> {
        Err(CreationError::Db(
            "core signal seam: execution_fields runs only past a state change".to_owned(),
        )
        .into())
    }

    async fn lock_cloud_creation_capacity(
        &mut self,
        _workspace_id: Uuid,
        _executor_kind: AgentExecutorKind,
        _automatic: bool,
    ) -> Result<Option<AdmissionError>, CreationError> {
        seam_unreachable("lock_cloud_creation_capacity")
    }

    fn dispatch_after_commit(&mut self, _run_id: Uuid) {}

    async fn render_bundle(
        &mut self,
        _issue_id: Uuid,
        _run_id: Uuid,
        _parent_run_id: Option<Uuid>,
        _trigger: &str,
        _created_by_id: Uuid,
    ) -> Result<RenderBundle, CreationError> {
        seam_unreachable("render_bundle")
    }

    fn extra_toolsets_schema_tool(&self) -> String {
        String::new()
    }
}

impl FinalizeAgentRunSeam for CoreSignalSeam<'_> {
    async fn finalize_failed_run(
        &mut self,
        _run_id: Uuid,
        _error_code: &str,
        _error: &str,
        _now: DateTime<Utc>,
    ) -> Result<RunView, CreationError> {
        seam_unreachable("finalize_failed_run")
    }
}

impl EntriesSeam for CoreSignalSeam<'_> {
    async fn prior_state_id(&mut self, issue_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        let row: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(PRIOR_STATE_SELECT_SQL)
            .bind(issue_id)
            .fetch_optional(self.pool)
            .await
            .map_err(|error| CreationError::Db(error.to_string()))?;
        Ok(row.and_then(|(_, state_id)| state_id))
    }

    async fn queued_follow_up(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        seam_unreachable("queued_follow_up")
    }

    async fn lock_ticker(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, CreationError> {
        seam_unreachable("lock_ticker")
    }

    async fn save_ticker(
        &mut self,
        _row: &IssueAgentTicker,
        _write: pidash_services::orchestration::clock::ClockWrite,
    ) -> Result<(), CreationError> {
        seam_unreachable("save_ticker")
    }

    fn set_rollback(&mut self) {}

    async fn clock_policy(
        &mut self,
        _project_id: Uuid,
    ) -> Result<ProjectClockPolicy, CreationError> {
        seam_unreachable("clock_policy")
    }

    async fn binding(
        &mut self,
        _binding_id: Uuid,
    ) -> Result<pidash_services::orchestration::entries::BindingView, CreationError> {
        seam_unreachable("binding")
    }

    async fn scheduler_override_pod(
        &mut self,
        _pod_id: Uuid,
        _project_id: Option<Uuid>,
    ) -> Result<Option<PodView>, CreationError> {
        seam_unreachable("scheduler_override_pod")
    }

    async fn workspace(
        &mut self,
        _workspace_id: Uuid,
    ) -> Result<pidash_services::orchestration::entries::WorkspaceView, CreationError> {
        seam_unreachable("workspace")
    }

    async fn scheduler_row(
        &mut self,
        _scheduler_id: Uuid,
    ) -> Result<pidash_services::orchestration::entries::SchedulerView, CreationError> {
        seam_unreachable("scheduler_row")
    }

    async fn scheduler_override_rows(
        &mut self,
        _workspace_id: Uuid,
    ) -> Result<Vec<OverrideRow>, CreationError> {
        seam_unreachable("scheduler_override_rows")
    }

    async fn project_role_facts(
        &mut self,
        _user_id: Uuid,
        _workspace_slug: &str,
        _project_id: Uuid,
    ) -> Result<ProjectRoleFacts, CreationError> {
        seam_unreachable("project_role_facts")
    }

    async fn has_usable_llm_config(&mut self, _user_id: Uuid) -> Result<bool, CreationError> {
        seam_unreachable("has_usable_llm_config")
    }

    async fn agent_system_user(
        &mut self,
    ) -> Result<
        Result<Uuid, pidash_db::orchestration::workpad::AgentUserCollisionError>,
        CreationError,
    > {
        seam_unreachable("agent_system_user")
    }

    async fn insert_scheduler_run(
        &mut self,
        _row: &pidash_services::orchestration::entries::NewSchedulerRun,
    ) -> Result<RunView, CreationError> {
        seam_unreachable("insert_scheduler_run")
    }

    fn compose_scheduler_turn(
        &mut self,
        _context: &Value,
        _index: &pidash_services::prompting::composer::OverrideIndex,
        _workspace_id: Option<&str>,
        _executor_kind: Option<&str>,
        _tool_catalog_version: i64,
    ) -> Result<RenderedTurn, String> {
        Err("core signal seam: compose_scheduler_turn runs only past a state change".to_owned())
    }
}

impl PreflightSeam for CorePreflight {
    async fn preflight_eligibility_or_bounce(
        &mut self,
        _issue_id: Uuid,
        _creator_id: Uuid,
        _pod_id: Uuid,
        _triggered_by: &str,
    ) -> Result<bool, CreationError> {
        seam_unreachable("preflight_eligibility_or_bounce")
    }
}

// ---------------------------------------------------------------------------
// `Issue.save()` port: defaults, recomputes, sequence/sort, the write
// ---------------------------------------------------------------------------

/// The state field `get()`: same guards as [`pk_exists`], but the row's
/// group rides along for the save's `completed_at` recompute.
async fn state_row(
    pool: &sqlx::PgPool,
    queryset: PkQueryset,
    id: &Uuid,
) -> Result<Option<(Uuid, String)>, sqlx::Error> {
    let sql = match queryset {
        PkQueryset::StatesAll => "SELECT id, \"group\" FROM states WHERE id = $1",
        PkQueryset::States => {
            "SELECT id, \"group\" FROM states WHERE id = $1 AND deleted_at IS NULL AND \"group\" != 'triage'"
        }
        _ => return Ok(None),
    };
    sqlx::query_as(sql).bind(id).fetch_optional(pool).await
}

/// The NULL-state default assignment (`issue.py:288-301`): the default
/// state first, else the first non-triage state, both `ORDER BY sequence`.
/// Predicates are the `State.objects` manager (live + non-triage group)
/// plus the explicit `~is_triage` filter — `is_triage` is a plain boolean
/// with no save-sync, so both predicates stay.
async fn default_state_for_project(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
) -> Result<Option<Uuid>, Denial> {
    for default_only in [true, false] {
        let extra = if default_only {
            " AND s.\"default\""
        } else {
            ""
        };
        let sql = format!(
            "SELECT s.id FROM states AS s WHERE s.project_id = $1 \
             AND s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage') AND NOT s.is_triage{extra} \
             ORDER BY s.sequence ASC LIMIT 1"
        );
        let row: Option<(Uuid,)> = sqlx::query_as(&sql)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        if let Some((id,)) = row {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// `Pod.default_for_project_id` (`runner/models.py:174-176`):
/// `Pod.objects` (live only), project-default first.
async fn default_pod_for_project(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
) -> Result<Option<Uuid>, Denial> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM pod WHERE project_id = $1 AND is_default AND deleted_at IS NULL \
         ORDER BY is_default DESC, created_at ASC LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.0))
}

/// `self.state` on a save whose state id Django must resolve
/// (untouched-state updates, destroy): the `State.objects` manager, so a
/// triage/deleted id raises `DoesNotExist` → the required-object 404.
async fn saved_state_group(pool: &sqlx::PgPool, state_id: &Uuid) -> Result<Option<String>, Denial> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT \"group\" FROM states WHERE id = $1 AND deleted_at IS NULL AND NOT (\"group\" = 'triage')",
    )
    .bind(state_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.0))
}

/// `self.project` on a save (`issue.py:293-296` + the lock key): the
/// issue's FK cache is cold on both paths, so Django fetches the row. A
/// miss raises `DoesNotExist` → the required-object 404.
async fn touch_save_project(pool: &sqlx::PgPool, project_id: &Uuid) -> Result<(), Denial> {
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM projects WHERE id = $1 AND deleted_at IS NULL")
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    row.map(|_| ()).ok_or(Denial::NotFound)
}

/// `convert_uuid_to_integer` (`utils/uuid.py:19-26`): sha256 of the
/// hyphenated-lowercase id, first 8 bytes big-endian signed.
fn advisory_lock_key(project_id: &Uuid) -> i64 {
    let mut hasher = Sha256::new();
    hasher.update(project_id.to_string().as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes)
}

/// Every `issues` column in [`models_core`](pidash_db::app_issues::models_core)
/// `COLUMNS` order, as owned values — the INSERT and the full-column
/// UPDATE both bind from here.
struct IssueSaveRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    deleted_at: Option<DateTime<Utc>>,
    project_id: Uuid,
    workspace_id: Uuid,
    parent_id: Option<Uuid>,
    state_id: Option<Uuid>,
    point: Option<i32>,
    estimate_point_id: Option<Uuid>,
    name: String,
    description_json: Value,
    description_html: String,
    description_stripped: Option<String>,
    description_binary: Option<Vec<u8>>,
    priority: String,
    complexity_score: i32,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    sequence_id: i32,
    sort_order: f64,
    completed_at: Option<DateTime<Utc>>,
    archived_at: Option<NaiveDate>,
    is_draft: bool,
    external_source: Option<String>,
    external_id: Option<String>,
    type_id: Option<Uuid>,
    git_work_branch: String,
    workpad: String,
    created_via: Option<String>,
    assigned_pod_id: Option<Uuid>,
    agent_executor: Option<String>,
}

/// `Issue.objects.create(...)`: the 34-column INSERT in `COLUMNS` order.
async fn insert_issue_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    row: &IssueSaveRow,
) -> Result<(), Denial> {
    sqlx::query(
        "INSERT INTO issues (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
         project_id, workspace_id, parent_id, state_id, point, estimate_point_id, name, \
         description_json, description_html, description_stripped, description_binary, priority, \
         complexity_score, start_date, target_date, sequence_id, sort_order, completed_at, \
         archived_at, is_draft, external_source, external_id, type_id, git_work_branch, workpad, \
         created_via, assigned_pod_id, agent_executor) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, \
         $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, $32, $33, $34)",
    )
    .bind(row.id)
    .bind(row.created_at)
    .bind(row.updated_at)
    .bind(row.created_by_id)
    .bind(row.updated_by_id)
    .bind(row.deleted_at)
    .bind(row.project_id)
    .bind(row.workspace_id)
    .bind(row.parent_id)
    .bind(row.state_id)
    .bind(row.point)
    .bind(row.estimate_point_id)
    .bind(row.name.as_str())
    .bind(row.description_json.clone())
    .bind(row.description_html.as_str())
    .bind(row.description_stripped.as_deref())
    .bind(row.description_binary.as_deref())
    .bind(row.priority.as_str())
    .bind(row.complexity_score)
    .bind(row.start_date)
    .bind(row.target_date)
    .bind(row.sequence_id)
    .bind(row.sort_order)
    .bind(row.completed_at)
    .bind(row.archived_at)
    .bind(row.is_draft)
    .bind(row.external_source.as_deref())
    .bind(row.external_id.as_deref())
    .bind(row.type_id)
    .bind(row.git_work_branch.as_str())
    .bind(row.workpad.as_str())
    .bind(row.created_via.as_deref())
    .bind(row.assigned_pod_id)
    .bind(row.agent_executor.as_deref())
    .execute(&mut **tx)
    .await
    .map_err(|error| {
        if is_integrity_violation(&error) {
            Denial::BadError("The payload is not valid".to_owned())
        } else {
            Denial::ServerError
        }
    })?;
    Ok(())
}

/// `instance.save()` on an update: Django writes every column (no
/// `update_fields`), so the UPDATE sets all 33 non-pk columns.
async fn update_issue_row(pool: &sqlx::PgPool, row: &IssueSaveRow) -> Result<(), Denial> {
    sqlx::query(
        "UPDATE issues SET created_at = $1, updated_at = $2, created_by_id = $3, updated_by_id = $4, \
         deleted_at = $5, project_id = $6, workspace_id = $7, parent_id = $8, state_id = $9, \
         point = $10, estimate_point_id = $11, name = $12, description_json = $13, \
         description_html = $14, description_stripped = $15, description_binary = $16, \
         priority = $17, complexity_score = $18, start_date = $19, target_date = $20, \
         sequence_id = $21, sort_order = $22, completed_at = $23, archived_at = $24, \
         is_draft = $25, external_source = $26, external_id = $27, type_id = $28, \
         git_work_branch = $29, workpad = $30, created_via = $31, assigned_pod_id = $32, \
         agent_executor = $33 WHERE id = $34",
    )
    .bind(row.created_at)
    .bind(row.updated_at)
    .bind(row.created_by_id)
    .bind(row.updated_by_id)
    .bind(row.deleted_at)
    .bind(row.project_id)
    .bind(row.workspace_id)
    .bind(row.parent_id)
    .bind(row.state_id)
    .bind(row.point)
    .bind(row.estimate_point_id)
    .bind(row.name.as_str())
    .bind(row.description_json.clone())
    .bind(row.description_html.as_str())
    .bind(row.description_stripped.as_deref())
    .bind(row.description_binary.as_deref())
    .bind(row.priority.as_str())
    .bind(row.complexity_score)
    .bind(row.start_date)
    .bind(row.target_date)
    .bind(row.sequence_id)
    .bind(row.sort_order)
    .bind(row.completed_at)
    .bind(row.archived_at)
    .bind(row.is_draft)
    .bind(row.external_source.as_deref())
    .bind(row.external_id.as_deref())
    .bind(row.type_id)
    .bind(row.git_work_branch.as_str())
    .bind(row.workpad.as_str())
    .bind(row.created_via.as_deref())
    .bind(row.assigned_pod_id)
    .bind(row.agent_executor.as_deref())
    .bind(row.id)
    .execute(pool)
    .await
    .map_err(|error| {
        if is_integrity_violation(&error) {
            Denial::BadError("The payload is not valid".to_owned())
        } else {
            Denial::ServerError
        }
    })?;
    Ok(())
}

/// One m2m `bulk_create(batch_size=10)` leg: a multi-row INSERT per
/// 10-chunk in input order; an `IntegrityError` swallows the REST of the
/// batches (`except: pass` wraps the whole call), any other error 500s.
#[allow(clippy::too_many_arguments)]
async fn write_m2m_batches(
    pool: &sqlx::PgPool,
    table: &str,
    member_column: &str,
    ids: &[Uuid],
    issue_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    ignore_conflicts: bool,
) -> Result<(), Denial> {
    for batch in m2m_batches(ids) {
        let sql = m2m_insert_sql(table, member_column, batch.len(), ignore_conflicts);
        let mut query = sqlx::query(&sql);
        for member_id in batch {
            // `auto_now_add` + `auto_now`: two `now()` calls per row, like
            // the per-object instantiation Django bulk-saves.
            let created = utc_now_micros();
            let updated = utc_now_micros();
            query = query
                .bind(Uuid::new_v4())
                .bind(created)
                .bind(updated)
                .bind(created_by_id)
                .bind(updated_by_id)
                .bind(project_id)
                .bind(workspace_id)
                .bind(issue_id)
                .bind(member_id);
        }
        match query.execute(pool).await {
            Ok(_) => {}
            Err(error) if is_integrity_violation(&error) => break,
            Err(_) => return Err(Denial::ServerError),
        }
    }
    Ok(())
}

/// The create default-assignee fallback (`serializers/issue.py:419-444`):
/// empty/missing assignees + a valid project-default member → one
/// `IssueAssignee` row, `IntegrityError` swallowed.
async fn write_default_assignee(
    pool: &sqlx::PgPool,
    default_assignee_id: &Uuid,
    issue_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
    created_by_id: Option<Uuid>,
) -> Result<(), Denial> {
    let member: Option<(i32,)> = sqlx::query_as(DEFAULT_ASSIGNEE_EXISTS_SQL)
        .bind(default_assignee_id)
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if member.is_none() {
        return Ok(());
    }
    let sql = m2m_insert_sql("issue_assignees", "assignee_id", 1, false);
    let created = utc_now_micros();
    let updated = utc_now_micros();
    match sqlx::query(&sql)
        .bind(Uuid::new_v4())
        .bind(created)
        .bind(updated)
        .bind(created_by_id)
        .bind(None::<Uuid>)
        .bind(project_id)
        .bind(workspace_id)
        .bind(issue_id)
        .bind(default_assignee_id)
        .execute(pool)
        .await
    {
        Ok(_) => Ok(()),
        Err(error) if is_integrity_violation(&error) => Ok(()),
        Err(_) => Err(Denial::ServerError),
    }
}

// ---------------------------------------------------------------------------
// Task enqueues (best-effort: failures never change the response)
// ---------------------------------------------------------------------------

/// `task.delay(*args, **kwargs)` through the jobs kernel: unported tasks
/// forward Celery-format, like every other D-26 port. Enqueue failures
/// log and the response stands (Django's broker publish is equally
/// fire-and-forget from the view's perspective).
async fn enqueue_task(
    pool: &sqlx::PgPool,
    task: &str,
    args: Vec<Value>,
    kwargs: Map<String, Value>,
) {
    let message = CeleryTaskMessage::new(task, args, kwargs);
    let job = NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// `issue_activity.delay(type, requested_data, actor_id, issue_id,
/// project_id, current_instance, epoch, notification, origin[, subscriber])`.
#[allow(clippy::too_many_arguments)]
async fn enqueue_issue_activity(
    pool: &sqlx::PgPool,
    activity_type: &str,
    requested_data: Value,
    actor_id: &Uuid,
    issue_id: &Uuid,
    project_id: &Uuid,
    current_instance: Value,
    origin: &str,
    subscriber: Option<bool>,
) {
    let mut kwargs = Map::new();
    kwargs.insert("type".to_owned(), Value::String(activity_type.to_owned()));
    kwargs.insert("requested_data".to_owned(), requested_data);
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_string()));
    kwargs.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    kwargs.insert("current_instance".to_owned(), current_instance);
    kwargs.insert(
        "epoch".to_owned(),
        Value::Number(serde_json::Number::from(Utc::now().timestamp())),
    );
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    if let Some(subscriber) = subscriber {
        kwargs.insert("subscriber".to_owned(), Value::Bool(subscriber));
    }
    enqueue_task(pool, ISSUE_ACTIVITY_TASK, vec![], kwargs).await;
}

/// `model_activity.delay(model_name, model_id, requested_data,
/// current_instance, actor_id, slug, origin)`: `requested_data` is the RAW
/// object (never JSON-encoded at these sites); the UUID actor rides as a
/// string, exactly as Celery's encoder would carry the UUID object.
#[allow(clippy::too_many_arguments)]
async fn enqueue_model_activity(
    pool: &sqlx::PgPool,
    model_id: &Uuid,
    requested_data: Value,
    current_instance: Value,
    actor_id: &Uuid,
    slug: &str,
    origin: &str,
) {
    let mut kwargs = Map::new();
    kwargs.insert("model_name".to_owned(), Value::String("issue".to_owned()));
    kwargs.insert("model_id".to_owned(), Value::String(model_id.to_string()));
    kwargs.insert("requested_data".to_owned(), requested_data);
    kwargs.insert("current_instance".to_owned(), current_instance);
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_string()));
    kwargs.insert("slug".to_owned(), Value::String(slug.to_owned()));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    enqueue_task(pool, MODEL_ACTIVITY_TASK, vec![], kwargs).await;
}

/// `recent_visited_task.delay(slug, entity_name, entity_identifier,
/// user_id, project_id)` — all keywords.
async fn enqueue_recent_visited(
    pool: &sqlx::PgPool,
    slug: &str,
    entity_identifier: &str,
    user_id: &Uuid,
    project_id: &Uuid,
) {
    let mut kwargs = Map::new();
    kwargs.insert("slug".to_owned(), Value::String(slug.to_owned()));
    kwargs.insert("entity_name".to_owned(), Value::String("issue".to_owned()));
    kwargs.insert(
        "entity_identifier".to_owned(),
        Value::String(entity_identifier.to_owned()),
    );
    kwargs.insert("user_id".to_owned(), Value::String(user_id.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    enqueue_task(pool, RECENT_VISITED_TASK, vec![], kwargs).await;
}

/// `issue_description_version_task.delay(updated_issue, issue_id, user_id[,
/// is_creating])`: the update site OMITS `is_creating` (it is not `False`).
async fn enqueue_version_task(
    pool: &sqlx::PgPool,
    updated_issue: &str,
    issue_id: &Uuid,
    user_id: &Uuid,
    is_creating: Option<bool>,
) {
    let mut kwargs = Map::new();
    kwargs.insert(
        "updated_issue".to_owned(),
        Value::String(updated_issue.to_owned()),
    );
    kwargs.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    kwargs.insert("user_id".to_owned(), Value::String(user_id.to_string()));
    if let Some(is_creating) = is_creating {
        kwargs.insert("is_creating".to_owned(), Value::Bool(is_creating));
    }
    enqueue_task(pool, VERSION_TASK, vec![], kwargs).await;
}

/// `soft_delete_related_objects.delay(app_label, model_name, pk,
/// using=None)`: positional ids + the explicit `using` kwarg.
async fn enqueue_soft_delete(pool: &sqlx::PgPool, issue_id: &Uuid) {
    let mut kwargs = Map::new();
    kwargs.insert("using".to_owned(), Value::Null);
    enqueue_task(
        pool,
        SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String("issue".to_owned()),
            Value::String(issue_id.to_string()),
        ],
        kwargs,
    )
    .await;
}

// ---------------------------------------------------------------------------
// Datetime rendering (byte-exact)
// ---------------------------------------------------------------------------

/// `user_timezone_converter` plus DRF's `JSONEncoder`: the value in the
/// actor's timezone, `isoformat()`, with a UTC `+00:00` rewritten to `Z`.
fn py_iso_in(moment: &DateTime<Utc>, timezone: &Tz) -> String {
    let local = moment.with_timezone(timezone);
    let base = local.format("%Y-%m-%dT%H:%M:%S").to_string();
    let micros = local.timestamp_subsec_micros();
    let offset = local.format("%:z").to_string();
    let rendered = if micros == 0 {
        format!("{base}{offset}")
    } else {
        format!("{base}.{:06}{offset}", micros)
    };
    if rendered.ends_with("+00:00") {
        format!("{}Z", &rendered[..rendered.len() - 6])
    } else {
        rendered
    }
}

// ---------------------------------------------------------------------------
// Row fetchers: the lookup SELECTs per path
// ---------------------------------------------------------------------------

/// `assignee_ids` WITHOUT the active-member join: the create post-query
/// goes through `issue_queryset_grouper` (live links only), unlike the
/// retrieve / PATCH annotations.
const ASSIGNEE_IDS_PLAIN_SELECT: &str =
    "(SELECT COALESCE(ARRAY_AGG(DISTINCT ia.assignee_id), '{}'::uuid[]) \
    FROM issue_assignees ia WHERE ia.issue_id = issue.id AND ia.deleted_at IS NULL) AS assignee_ids";

/// The `issue_objects` scope shared by the PATCH/PUT/create-response
/// lookups: live rows, non-triage state (NULL-state rows KEPT — the
/// pilot-2 null-safe form), unarchived issue + project, non-draft.
/// Needs the `state` + `project` joins the builder adds.
const ISSUE_OBJECTS_SCOPE: &str =
    "AND NOT (state.\"group\" = 'triage' AND state.\"group\" IS NOT NULL) \
     AND issue.archived_at IS NULL AND project.archived_at IS NULL AND issue.is_draft = FALSE";

/// One looked-up issue: the 34 storable columns, the scalar annotations
/// (each `None`/empty when its leg is absent), and the joined state pair
/// for the ticker leg.
struct CoreIssueRow {
    save: IssueSaveRow,
    cycle_id: Option<Uuid>,
    link_count: Option<i64>,
    attachment_count: Option<i64>,
    sub_issues_count: Option<i64>,
    label_ids: Vec<Uuid>,
    assignee_ids: Vec<Uuid>,
    module_ids: Vec<Uuid>,
    is_subscribed: bool,
    state_group: Option<String>,
    state_name: Option<String>,
}

/// Map the 34 storable columns off a `SELECT {ISSUE_COLUMNS}` row.
fn save_row_from_pg(row: &sqlx::postgres::PgRow) -> Result<IssueSaveRow, sqlx::Error> {
    Ok(IssueSaveRow {
        id: row.try_get("id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        deleted_at: row.try_get("deleted_at")?,
        project_id: row.try_get("project_id")?,
        workspace_id: row.try_get("workspace_id")?,
        parent_id: row.try_get("parent_id")?,
        state_id: row.try_get("state_id")?,
        point: row.try_get("point")?,
        estimate_point_id: row.try_get("estimate_point_id")?,
        name: row.try_get("name")?,
        description_json: row.try_get("description_json")?,
        description_html: row.try_get("description_html")?,
        description_stripped: row.try_get("description_stripped")?,
        description_binary: row.try_get("description_binary")?,
        priority: row.try_get("priority")?,
        complexity_score: row.try_get("complexity_score")?,
        start_date: row.try_get("start_date")?,
        target_date: row.try_get("target_date")?,
        sequence_id: row.try_get("sequence_id")?,
        sort_order: row.try_get("sort_order")?,
        completed_at: row.try_get("completed_at")?,
        archived_at: row.try_get("archived_at")?,
        is_draft: row.try_get("is_draft")?,
        external_source: row.try_get("external_source")?,
        external_id: row.try_get("external_id")?,
        type_id: row.try_get("type_id")?,
        git_work_branch: row.try_get("git_work_branch")?,
        workpad: row.try_get("workpad")?,
        created_via: row.try_get("created_via")?,
        assigned_pod_id: row.try_get("assigned_pod_id")?,
        agent_executor: row.try_get("agent_executor")?,
    })
}

/// The annotated issue lookup: retrieve's queryset (live-only scope +
/// member-active assignees + `is_subscribed`) or the PATCH shape
/// (`issue_objects` scope, no `is_subscribed`). `.first()` applies the
/// `-created_at` ordering; the PATCH leg adds the queryset's `DISTINCT`
/// (a no-op past the pk filter, kept for text fidelity). PATCH's own
/// `ArrayAgg` annotations take the correlated-subquery form here — same
/// multisets, hence same arrays modulo Postgres's unspecified `ARRAY_AGG`
/// order on both sides.
async fn fetch_annotated_issue(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    user_id: &Uuid,
    issue_objects: bool,
) -> Result<Option<CoreIssueRow>, Denial> {
    let link = link_count_select(false);
    let attachment = attachment_count_select(false);
    let sub = sub_issues_count_select(false);
    let (distinct, scope, joins) = if issue_objects {
        (
            "DISTINCT ",
            ISSUE_OBJECTS_SCOPE,
            "JOIN projects AS project ON project.id = issue.project_id",
        )
    } else {
        ("", "", "")
    };
    let subscribed = if issue_objects {
        "FALSE AS is_subscribed".to_owned()
    } else {
        "EXISTS(SELECT 1 AS a FROM issue_subscribers s \
         JOIN workspaces sw ON sw.id = s.workspace_id \
         WHERE s.deleted_at IS NULL AND s.issue_id = issue.id AND s.project_id = $4 \
         AND s.subscriber_id = $5 AND sw.slug = $6 LIMIT 1) AS is_subscribed"
            .to_owned()
    };
    let sql = format!(
        "SELECT {distinct}{ISSUE_COLUMNS}, {CYCLE_ID_SELECT}, {link}, {attachment}, {sub}, \
         {LABEL_IDS_SELECT}, {ASSIGNEE_IDS_ACTIVE_SELECT}, {MODULE_IDS_SELECT}, {subscribed}, \
         state.\"group\" AS state_group, state.name AS state_name \
         FROM issues AS issue \
         JOIN workspaces ON workspaces.id = issue.workspace_id \
         LEFT JOIN states AS state ON state.id = issue.state_id \
         {joins} \
         WHERE issue.deleted_at IS NULL AND issue.id = $1 \
         AND issue.project_id = $2 AND workspaces.slug = $3 {scope} \
         ORDER BY issue.created_at DESC LIMIT 1"
    );
    let mut query = sqlx::query(&sql).bind(issue_id).bind(project_id).bind(slug);
    if !issue_objects {
        query = query.bind(project_id).bind(user_id).bind(slug);
    }
    let row: Option<sqlx::postgres::PgRow> = query
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let save = save_row_from_pg(&row).map_err(|_| Denial::ServerError)?;
    Ok(Some(CoreIssueRow {
        save,
        cycle_id: row.try_get("cycle_id").map_err(|_| Denial::ServerError)?,
        link_count: row.try_get("link_count").map_err(|_| Denial::ServerError)?,
        attachment_count: row
            .try_get("attachment_count")
            .map_err(|_| Denial::ServerError)?,
        sub_issues_count: row
            .try_get("sub_issues_count")
            .map_err(|_| Denial::ServerError)?,
        label_ids: row.try_get("label_ids").map_err(|_| Denial::ServerError)?,
        assignee_ids: row
            .try_get("assignee_ids")
            .map_err(|_| Denial::ServerError)?,
        module_ids: row.try_get("module_ids").map_err(|_| Denial::ServerError)?,
        is_subscribed: row
            .try_get("is_subscribed")
            .map_err(|_| Denial::ServerError)?,
        state_group: row
            .try_get("state_group")
            .map_err(|_| Denial::ServerError)?,
        state_name: row.try_get("state_name").map_err(|_| Denial::ServerError)?,
    }))
}

/// `Issue.objects.get(workspace__slug, project_id, pk)`: the destroy
/// lookup — live-only, no annotations, no ordering (`.get()` applies
/// none).
async fn fetch_destroy_issue(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<Option<IssueSaveRow>, Denial> {
    let sql = format!(
        "SELECT {ISSUE_COLUMNS} FROM issues AS issue \
         JOIN workspaces ON workspaces.id = issue.workspace_id \
         WHERE issue.deleted_at IS NULL AND issue.id = $1 \
         AND issue.project_id = $2 AND workspaces.slug = $3"
    );
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(issue_id)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| save_row_from_pg(&row).map_err(|_| Denial::ServerError))
        .transpose()
}

/// The PUT lookup: the plain `get_queryset` (`issue_objects`, no
/// annotations — DRF's `get_object` never applies them).
async fn fetch_put_issue(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<Option<IssueSaveRow>, Denial> {
    let sql = format!(
        "SELECT DISTINCT {ISSUE_COLUMNS} FROM issues AS issue \
         JOIN workspaces ON workspaces.id = issue.workspace_id \
         LEFT JOIN states AS state ON state.id = issue.state_id \
         JOIN projects AS project ON project.id = issue.project_id \
         WHERE issue.deleted_at IS NULL AND issue.id = $1 \
         AND issue.project_id = $2 AND workspaces.slug = $3 {ISSUE_OBJECTS_SCOPE} \
         ORDER BY issue.created_at DESC LIMIT 1"
    );
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(issue_id)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| save_row_from_pg(&row).map_err(|_| Denial::ServerError))
        .transpose()
}

impl CoreIssueRow {
    /// The `validate()`-consumed snapshot of the looked-up row.
    fn write_instance(&self) -> WriteInstance {
        let save = &self.save;
        WriteInstance {
            id: save.id,
            project_id: save.project_id,
            workspace_id: save.workspace_id,
            external_source: save.external_source.clone(),
            agent_executor: save.agent_executor.clone(),
            assigned_pod_id: save.assigned_pod_id,
            name: save.name.clone(),
            description_html: save.description_html.clone(),
            description_json: save.description_json.clone(),
            description_stripped: save.description_stripped.clone(),
        }
    }
}

impl IssueSaveRow {
    /// The `validate()`-consumed snapshot (annotation-less lookups).
    fn write_instance(&self) -> WriteInstance {
        WriteInstance {
            id: self.id,
            project_id: self.project_id,
            workspace_id: self.workspace_id,
            external_source: self.external_source.clone(),
            agent_executor: self.agent_executor.clone(),
            assigned_pod_id: self.assigned_pod_id,
            name: self.name.clone(),
            description_html: self.description_html.clone(),
            description_json: self.description_json.clone(),
            description_stripped: self.description_stripped.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Detail assembly: ticker / runs / blockers / `IssueDetailSerializer`
// ---------------------------------------------------------------------------

/// The reaction + link prefetches (`base.py:559-570`): retrieve issues
/// both (with their `select_related` joins) regardless of `expand`.
/// Results are discarded — no expand leg consumes them — so these are
/// count-parity selects over the same tables and live guards.
async fn touch_detail_prefetches(pool: &sqlx::PgPool, issue_id: &Uuid) -> Result<(), Denial> {
    sqlx::query("SELECT id FROM issue_reactions WHERE issue_id = $1 AND deleted_at IS NULL")
        .bind(issue_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    sqlx::query("SELECT id FROM issue_links WHERE issue_id = $1 AND deleted_at IS NULL")
        .bind(issue_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `obj.agent_ticker` (`issue.py:1275-1281`): the reverse OneToOne read —
/// a miss renders `agent_ticker: null` (and, without runs, a null
/// `agent_status` too).
async fn fetch_ticker(
    pool: &sqlx::PgPool,
    issue_id: &Uuid,
) -> Result<Option<IssueAgentTicker>, Denial> {
    let columns = TICKER_COLUMNS.join(", ");
    let sql =
        format!("SELECT {columns} FROM {TICKER_TABLE} WHERE issue_id = $1 AND deleted_at IS NULL");
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let get = |name: &str| -> Result<String, Denial> {
        row.try_get(name).map_err(|_| Denial::ServerError)
    };
    Ok(Some(IssueAgentTicker {
        id: row.try_get("id").map_err(|_| Denial::ServerError)?,
        created_at: row.try_get("created_at").map_err(|_| Denial::ServerError)?,
        updated_at: row.try_get("updated_at").map_err(|_| Denial::ServerError)?,
        created_by_id: row
            .try_get("created_by_id")
            .map_err(|_| Denial::ServerError)?,
        updated_by_id: row
            .try_get("updated_by_id")
            .map_err(|_| Denial::ServerError)?,
        deleted_at: row.try_get("deleted_at").map_err(|_| Denial::ServerError)?,
        issue_id: row.try_get("issue_id").map_err(|_| Denial::ServerError)?,
        used: row.try_get("used").map_err(|_| Denial::ServerError)?,
        granted: row.try_get("granted").map_err(|_| Denial::ServerError)?,
        waited: row.try_get("waited").map_err(|_| Denial::ServerError)?,
        user_disabled: row
            .try_get("user_disabled")
            .map_err(|_| Denial::ServerError)?,
        next_run_at: row
            .try_get("next_run_at")
            .map_err(|_| Denial::ServerError)?,
        last_tick_at: row
            .try_get("last_tick_at")
            .map_err(|_| Denial::ServerError)?,
        enabled: row.try_get("enabled").map_err(|_| Denial::ServerError)?,
        disarm_reason: get("disarm_reason")?,
        pending_entry: row
            .try_get("pending_entry")
            .map_err(|_| Denial::ServerError)?,
        pending_entry_free: row
            .try_get("pending_entry_free")
            .map_err(|_| Denial::ServerError)?,
        pending_entry_actor_id: row
            .try_get("pending_entry_actor_id")
            .map_err(|_| Denial::ServerError)?,
        pending_entry_trigger: get("pending_entry_trigger")?,
        resume_parent_run_id: row
            .try_get("resume_parent_run_id")
            .map_err(|_| Denial::ServerError)?,
    }))
}

/// The clock policy behind `effective_max_ticks` /
/// `effective_interval_seconds` (`ticker.issue.project`): the consumed
/// project columns, read only past a ticker row. (Django re-fetches the
/// issue + state intermediaries on the way; those rows are already in
/// hand with identical values, so the reads narrow to the project.)
async fn fetch_clock_policy(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
) -> Result<ProjectClockPolicy, Denial> {
    type ClockRow = (
        Option<bool>,
        Option<i32>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
    );
    let row: Option<ClockRow> = sqlx::query_as(
        "SELECT agent_ticking_enabled, agent_default_max_ticks, agent_default_interval_seconds, \
             agent_review_default_interval_seconds, agent_test_default_interval_seconds \
             FROM projects WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // A missing project row raises on Django (`ticker.issue.project`
    // re-fetches through the live-only manager) → 500, never a default.
    let Some((ticking, max_ticks, interval, review_interval, test_interval)) = row else {
        return Err(Denial::ServerError);
    };
    Ok(ProjectClockPolicy {
        agent_ticking_enabled: ticking,
        agent_default_max_ticks: max_ticks,
        agent_default_interval_seconds: interval,
        agent_review_default_interval_seconds: review_interval,
        agent_test_default_interval_seconds: test_interval,
    })
}

/// A run row with every wire string pre-rendered (datetimes via
/// `_serialize_datetime`, i.e. UTC `isoformat`).
struct RenderedRun {
    id: String,
    status: String,
    executor_kind: String,
    queue_position: Option<i16>,
    runner_id: Option<String>,
    runner_name: Option<String>,
    created_at: String,
    assigned_at: Option<String>,
    started_at: Option<String>,
    ended_at: Option<String>,
    done_payload: Option<Value>,
    error: String,
    error_code: String,
    llm_model: String,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    live: Option<RenderedLive>,
}

struct RenderedLive {
    observed_run_id: Option<String>,
    last_event_at: Option<String>,
    last_event_kind: Option<String>,
    last_event_summary: Option<String>,
    agent_pid: Option<i32>,
    agent_subprocess_alive: Option<bool>,
    approvals_pending: Option<i32>,
    usage: Value,
    llm_model: Option<String>,
    turn_count: Option<i32>,
    updated_at: String,
}

/// One `AgentRun` row off the `LATEST/ACTIVE_AGENT_RUN_SQL` projection.
/// The live-state leg mirrors `_serialize_agent_run`: a runner-less run
/// carries none, and a live row observed on another run is dropped.
fn rendered_run_from_pg(
    row: &sqlx::postgres::PgRow,
    include_live_state: bool,
) -> Result<RenderedRun, sqlx::Error> {
    let opt_moment = |name: &str| -> Result<Option<String>, sqlx::Error> {
        let moment: Option<DateTime<Utc>> = row.try_get(name)?;
        Ok(moment.map(serialize_iso_datetime))
    };
    let id: Uuid = row.try_get("id")?;
    let runner_id: Option<Uuid> = row.try_get("runner_id")?;
    let live_updated: Option<DateTime<Utc>> = row.try_get("updated_at")?;
    let observed: Option<Uuid> = row.try_get("observed_run_id")?;
    let live = if include_live_state && runner_id.is_some() {
        match (live_updated, observed) {
            (Some(updated), observed) if observed.is_none_or(|seen| seen == id) => {
                let usage: Value = row.try_get("usage")?;
                // The projection carries TWO `llm_model` columns (run at
                // 12, live-state at 25): by-name reads take the run's, so
                // the live one reads positionally.
                let live_llm: Option<String> = row.try_get(25usize)?;
                Some(RenderedLive {
                    observed_run_id: observed.map(|seen| seen.to_string()),
                    last_event_at: opt_moment("last_event_at")?,
                    last_event_kind: row.try_get("last_event_kind")?,
                    last_event_summary: row.try_get("last_event_summary")?,
                    agent_pid: row.try_get("agent_pid")?,
                    agent_subprocess_alive: row.try_get("agent_subprocess_alive")?,
                    approvals_pending: row.try_get("approvals_pending")?,
                    usage,
                    llm_model: live_llm,
                    turn_count: row.try_get("turn_count")?,
                    updated_at: serialize_iso_datetime(updated),
                })
            }
            _ => None,
        }
    } else {
        None
    };
    let created: DateTime<Utc> = row.try_get("created_at")?;
    let run_llm: String = row.try_get("llm_model")?;
    Ok(RenderedRun {
        id: id.to_string(),
        status: row.try_get("status")?,
        executor_kind: row.try_get("executor_kind")?,
        queue_position: row.try_get("queue_position")?,
        runner_id: runner_id.map(|runner| runner.to_string()),
        runner_name: row.try_get("name")?,
        created_at: serialize_iso_datetime(created),
        assigned_at: opt_moment("assigned_at")?,
        started_at: opt_moment("started_at")?,
        ended_at: opt_moment("ended_at")?,
        done_payload: row.try_get("done_payload")?,
        error: row.try_get("error")?,
        error_code: row.try_get("error_code")?,
        llm_model: run_llm,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        live,
    })
}

async fn fetch_agent_runs(
    pool: &sqlx::PgPool,
    issue_id: &Uuid,
) -> Result<(Option<RenderedRun>, Option<RenderedRun>, i64), Denial> {
    let latest: Option<sqlx::postgres::PgRow> = sqlx::query(LATEST_AGENT_RUN_SQL)
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let active: Option<sqlx::postgres::PgRow> = sqlx::query(ACTIVE_AGENT_RUN_SQL)
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let count: (i64,) = sqlx::query_as(AGENT_RUN_COUNT_SQL)
        .bind(issue_id)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let active = active
        .map(|row| rendered_run_from_pg(&row, true))
        .transpose()
        .map_err(|_| Denial::ServerError)?;
    // The latest run carries live state only when no run is active
    // (`include_live_state=active_run is None`).
    let latest = latest
        .map(|row| rendered_run_from_pg(&row, active.is_none()))
        .transpose()
        .map_err(|_| Denial::ServerError)?;
    Ok((latest, active, count.0))
}

/// The cached blocker summary (`relations_summary(obj)`): both direction
/// lists plus the open-blockers `Exists`, in that order.
async fn fetch_blocker_summary(
    pool: &sqlx::PgPool,
    issue_id: &Uuid,
) -> Result<pidash_services::orchestration::blockers::RelationsSummary, Denial> {
    let fetch_list = async |blocking: bool| -> Result<Vec<BlockerRow>, Denial> {
        let sql = summary_sql(blocking).replace(":issue_id", "$1");
        type BlockerTuple = (Uuid, i32, String, Option<String>, Option<String>);
        let rows: Vec<BlockerTuple> = sqlx::query_as(&sql)
            .bind(issue_id)
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        Ok(rows
            .into_iter()
            .map(
                |(issue_id, sequence_id, project_identifier, state_name, state_group)| BlockerRow {
                    issue_id,
                    sequence_id,
                    project_identifier,
                    state_name,
                    state_group,
                },
            )
            .collect())
    };
    let blocked_by = fetch_list(false).await?;
    let blocking = fetch_list(true).await?;
    let open_sql = has_open_blockers_sql().replace(":issue_id", "$1");
    let open: Option<(i32,)> = sqlx::query_as(&open_sql)
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(relations_summary(&blocked_by, &blocking, open.is_some()))
}

/// Render one `IssueDetailSerializer(...).data` body: the 35-key retrieve
/// form, or the 34-key `current_instance` form without `is_subscribed`
/// (the PATCH queryset never annotates it, so DRF `SkipField`s the key).
/// `is_intake` is absent on both (neither queryset annotates it).
/// `current_instance` is `json.dumps` spaced (it is a dump of the dict,
/// not the response body), with the same key order.
async fn render_detail_body(
    pool: &sqlx::PgPool,
    row: &CoreIssueRow,
    timezone: &Tz,
    with_subscribed: bool,
    touch_prefetches: bool,
    spaced: bool,
) -> Result<String, Denial> {
    if touch_prefetches {
        touch_detail_prefetches(pool, &row.save.id).await?;
    }
    let ticker = fetch_ticker(pool, &row.save.id).await?;
    let policy = match ticker {
        Some(_) => fetch_clock_policy(pool, &row.save.project_id).await?,
        None => ProjectClockPolicy::default(),
    };
    let state_ref = match (row.state_group.as_deref(), row.state_name.as_deref()) {
        (Some(group), Some(name)) => Some(StateRef { group, name }),
        _ => None,
    };
    let (latest, active, run_count) = fetch_agent_runs(pool, &row.save.id).await?;
    let blockers = fetch_blocker_summary(pool, &row.save.id).await?;
    let synced = if row
        .save
        .external_source
        .as_deref()
        .is_some_and(|source| !source.is_empty())
    {
        let git = git_issue_synced(pool, &row.save.id)
            .await
            .map_err(|_| Denial::ServerError)?;
        git || github_issue_synced(pool, &row.save.id)
            .await
            .map_err(|_| Denial::ServerError)?
    } else {
        false
    };

    let save = &row.save;
    let id = save.id.to_string();
    let state_id = save.state_id.map(|value| value.to_string());
    let estimate_point = save.estimate_point_id.map(|value| value.to_string());
    let project_id = save.project_id.to_string();
    let parent_id = save.parent_id.map(|value| value.to_string());
    let cycle_id = row.cycle_id.map(|value| value.to_string());
    let assigned_pod_id = save.assigned_pod_id.map(|value| value.to_string());
    let module_ids: Vec<String> = row.module_ids.iter().map(ToString::to_string).collect();
    let label_ids: Vec<String> = row.label_ids.iter().map(ToString::to_string).collect();
    let assignee_ids: Vec<String> = row.assignee_ids.iter().map(ToString::to_string).collect();
    let module_refs: Vec<&str> = module_ids.iter().map(String::as_str).collect();
    let label_refs: Vec<&str> = label_ids.iter().map(String::as_str).collect();
    let assignee_refs: Vec<&str> = assignee_ids.iter().map(String::as_str).collect();
    // Base datetimes are DRF `DateTimeField` output: the request user's
    // zone (`enforce_timezone` over the `TimezoneMixin`-activated tz) with
    // `+00:00` rewritten to `Z`. Runs/ticker legs instead `_serialize_datetime`
    // (raw `.isoformat()`, always UTC `+00:00`) and bypass this.
    let created_at = render_datetime_in(&save.created_at, timezone);
    let updated_at = render_datetime_in(&save.updated_at, timezone);
    let completed_at = save
        .completed_at
        .map(|moment| render_datetime_in(&moment, timezone));
    let archived_at = save.archived_at.map(|date| date.to_string());
    let start_date = save.start_date.map(|date| date.to_string());
    let target_date = save.target_date.map(|date| date.to_string());
    let created_by = save.created_by_id.map(|value| value.to_string());
    let updated_by = save.updated_by_id.map(|value| value.to_string());
    let base = IssueDetailBaseRow {
        id: id.as_str(),
        name: save.name.as_str(),
        state_id: state_id.as_deref(),
        sort_order: save.sort_order,
        completed_at: completed_at.as_deref(),
        estimate_point: estimate_point.as_deref(),
        priority: save.priority.as_str(),
        complexity_score: save.complexity_score,
        start_date: start_date.as_deref(),
        target_date: target_date.as_deref(),
        sequence_id: save.sequence_id,
        project_id: project_id.as_str(),
        parent_id: parent_id.as_deref(),
        cycle_id: Some(cycle_id.as_deref()),
        assigned_pod_id: assigned_pod_id.as_deref(),
        agent_executor: save.agent_executor.as_deref(),
        module_ids: Some(module_refs),
        label_ids: Some(label_refs),
        assignee_ids: Some(assignee_refs),
        sub_issues_count: Some(row.sub_issues_count),
        created_at: created_at.as_str(),
        updated_at: updated_at.as_str(),
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        attachment_count: Some(row.attachment_count),
        link_count: Some(row.link_count),
        is_draft: save.is_draft,
        archived_at: archived_at.as_deref(),
        is_synced: synced,
    };
    let ticker_input = ticker.as_ref().map(|ticker| AgentTickerInput {
        ticker,
        policy: &policy,
        state: state_ref,
    });
    let latest_row = latest.as_ref().map(|run| AgentRunDetailRow {
        id: run.id.as_str(),
        status: run.status.as_str(),
        executor_kind: run.executor_kind.as_str(),
        queue_position: run.queue_position,
        runner_id: run.runner_id.as_deref(),
        runner_name: run.runner_name.as_deref(),
        created_at: run.created_at.as_str(),
        assigned_at: run.assigned_at.as_deref(),
        started_at: run.started_at.as_deref(),
        ended_at: run.ended_at.as_deref(),
        done_payload: run.done_payload.as_ref(),
        error: run.error.as_str(),
        error_code: run.error_code.as_str(),
        llm_model: run.llm_model.as_str(),
        input_tokens: run.input_tokens,
        output_tokens: run.output_tokens,
        total_tokens: run.total_tokens,
        live_state: run.live.as_ref().map(|live| AgentLiveStateRow {
            observed_run_id: live.observed_run_id.as_deref(),
            last_event_at: live.last_event_at.as_deref(),
            last_event_kind: live.last_event_kind.as_deref(),
            last_event_summary: live.last_event_summary.as_deref(),
            agent_pid: live.agent_pid,
            agent_subprocess_alive: live.agent_subprocess_alive,
            approvals_pending: live.approvals_pending,
            usage: &live.usage,
            llm_model: live.llm_model.as_deref(),
            turn_count: live.turn_count,
            updated_at: live.updated_at.as_str(),
        }),
    });
    let active_row = active.as_ref().map(|run| AgentRunDetailRow {
        id: run.id.as_str(),
        status: run.status.as_str(),
        executor_kind: run.executor_kind.as_str(),
        queue_position: run.queue_position,
        runner_id: run.runner_id.as_deref(),
        runner_name: run.runner_name.as_deref(),
        created_at: run.created_at.as_str(),
        assigned_at: run.assigned_at.as_deref(),
        started_at: run.started_at.as_deref(),
        ended_at: run.ended_at.as_deref(),
        done_payload: run.done_payload.as_ref(),
        error: run.error.as_str(),
        error_code: run.error_code.as_str(),
        llm_model: run.llm_model.as_str(),
        input_tokens: run.input_tokens,
        output_tokens: run.output_tokens,
        total_tokens: run.total_tokens,
        live_state: run.live.as_ref().map(|live| AgentLiveStateRow {
            observed_run_id: live.observed_run_id.as_deref(),
            last_event_at: live.last_event_at.as_deref(),
            last_event_kind: live.last_event_kind.as_deref(),
            last_event_summary: live.last_event_summary.as_deref(),
            agent_pid: live.agent_pid,
            agent_subprocess_alive: live.agent_subprocess_alive,
            approvals_pending: live.approvals_pending,
            usage: &live.usage,
            llm_model: live.llm_model.as_deref(),
            turn_count: live.turn_count,
            updated_at: live.updated_at.as_str(),
        }),
    });
    let detail = IssueDetailRow {
        base,
        description_html: save.description_html.as_str(),
        is_subscribed: row.is_subscribed,
        is_intake: None,
        ticker: ticker_input,
        latest_run: latest_row,
        active_run: active_row,
        run_count,
        blockers: &blockers,
    };
    let view = issue_detail_to_representation(&detail);
    let mut body = if spaced {
        to_spaced_json(&view)?
    } else {
        serde_json::to_string(&view).map_err(|_| Denial::ServerError)?
    };
    if !with_subscribed {
        // Drop the `is_subscribed` pair (it sits between
        // `description_html` and `agent_ticker`). The needle's unescaped
        // quotes cannot match inside any string value, so this excises
        // exactly the key DRF `SkipField`s.
        let needles = if spaced {
            [", \"is_subscribed\": true", ", \"is_subscribed\": false"]
        } else {
            [",\"is_subscribed\":true", ",\"is_subscribed\":false"]
        };
        for needle in needles {
            if let Some(start) = body.find(needle) {
                body.replace_range(start..start + needle.len(), "");
                break;
            }
        }
    }
    Ok(body)
}

// ---------------------------------------------------------------------------
// Validated-data application: `create()` / `update()` field mapping
// ---------------------------------------------------------------------------

/// Extractors over the field-pass map. `Null` appears only where the
/// field allows null (the field pass rejects it elsewhere), so `Null`
/// uniformly means "clear to `None`".
fn v_text<'a>(validated: &'a BTreeMap<&'static str, Validated>, key: &str) -> Option<&'a str> {
    match validated.get(key) {
        Some(Validated::Text(text)) => Some(text.as_str()),
        _ => None,
    }
}

fn v_uuid(validated: &BTreeMap<&'static str, Validated>, key: &str) -> Option<Uuid> {
    match validated.get(key) {
        Some(Validated::Uuid(id)) => Some(*id),
        Some(Validated::State(id, _)) => Some(*id),
        _ => None,
    }
}

fn v_uuid_list(validated: &BTreeMap<&'static str, Validated>, key: &str) -> Option<Vec<Uuid>> {
    match validated.get(key) {
        Some(Validated::UuidList(ids)) => Some(ids.clone()),
        _ => None,
    }
}

/// The dual-source state resolution: touched when either key is present,
/// the auto field winning, with its cached group for the save's
/// `completed_at` recompute.
fn v_state(validated: &BTreeMap<&'static str, Validated>) -> (bool, Option<Uuid>, Option<String>) {
    let touched = validated.contains_key("state") || validated.contains_key("state_id");
    let winning = validated.get("state").or_else(|| validated.get("state_id"));
    match winning {
        Some(Validated::State(id, group)) => (touched, Some(*id), Some(group.clone())),
        Some(Validated::Uuid(id)) => (touched, Some(*id), None),
        _ => (touched, None, None),
    }
}

/// The dual-source parent resolution (same precedence, no group needed).
fn v_parent(validated: &BTreeMap<&'static str, Validated>) -> Option<Uuid> {
    // Key precedence like `v_state`: a present-but-null `parent` clears,
    // it must not fall back to `parent_id`.
    if validated.contains_key("parent") {
        v_uuid(validated, "parent")
    } else {
        v_uuid(validated, "parent_id")
    }
}

/// The effective m2m list: the `validate()`-filtered ids when the input
/// was non-empty, else the raw input. Returns presence (absent input
/// leaves the leg untouched; present-but-empty clears) plus the ids.
fn v_m2m_effective(
    filtered: Option<Vec<Uuid>>,
    validated: &BTreeMap<&'static str, Validated>,
    key: &str,
) -> (bool, Vec<Uuid>) {
    let raw = v_uuid_list(validated, key);
    let present = raw.is_some();
    let effective = filtered.or(raw).unwrap_or_default();
    (present, effective)
}

/// The fresh-row assembly: the row, the touched state's group, and the raw
/// m2m inputs.
type NewIssueAssembly = (
    IssueSaveRow,
    Option<String>,
    Option<Vec<Uuid>>,
    Option<Vec<Uuid>>,
);

/// Build the fresh row for `Issue.objects.create(...)`: model defaults
/// everywhere the input is absent, the sanitized `description_html`, and
/// placeholders for the transaction legs (`sequence_id`, `sort_order`,
/// `completed_at`, `description_stripped`).
fn assemble_new_issue(
    validated: &BTreeMap<&'static str, Validated>,
    filtered: &ValidatedAttrs,
    tenant: &CoreTenant,
) -> Result<NewIssueAssembly, Denial> {
    let name = v_text(validated, "name").ok_or(Denial::ServerError)?;
    let (_, state_id, state_group) = v_state(validated);
    let description_html = filtered
        .description_html
        .clone()
        .or_else(|| v_text(validated, "description_html").map(str::to_owned))
        .unwrap_or_else(|| "<p></p>".to_owned());
    let now_created = utc_now_micros();
    let now_updated = utc_now_micros();
    let row = IssueSaveRow {
        id: Uuid::new_v4(),
        created_at: now_created,
        updated_at: now_updated,
        created_by_id: Some(tenant.user_id),
        updated_by_id: None,
        deleted_at: match validated.get("deleted_at") {
            Some(Validated::DateTime(moment)) => Some(*moment),
            _ => None,
        },
        project_id: tenant.project_id,
        workspace_id: tenant.workspace_id,
        parent_id: v_parent(validated),
        state_id,
        point: match validated.get("point") {
            Some(Validated::Int(point)) => Some(*point as i32),
            _ => None,
        },
        estimate_point_id: v_uuid(validated, "estimate_point"),
        name: name.to_owned(),
        description_json: match validated.get("description_json") {
            Some(Validated::Json(value)) => value.clone(),
            _ => Value::Object(Map::new()),
        },
        description_html,
        description_stripped: None,
        description_binary: None,
        priority: v_text(validated, "priority").unwrap_or("none").to_owned(),
        complexity_score: match validated.get("complexity_score") {
            Some(Validated::Int(score)) => *score as i32,
            _ => 0,
        },
        start_date: match validated.get("start_date") {
            Some(Validated::Date(date)) => Some(*date),
            _ => None,
        },
        target_date: match validated.get("target_date") {
            Some(Validated::Date(date)) => Some(*date),
            _ => None,
        },
        sequence_id: 0,
        sort_order: match validated.get("sort_order") {
            Some(Validated::Float(order)) => *order,
            _ => 65535.0,
        },
        completed_at: match validated.get("completed_at") {
            Some(Validated::DateTime(moment)) => Some(*moment),
            _ => None,
        },
        archived_at: match validated.get("archived_at") {
            Some(Validated::Date(date)) => Some(*date),
            _ => None,
        },
        is_draft: match validated.get("is_draft") {
            Some(Validated::Bool(draft)) => *draft,
            _ => false,
        },
        external_source: v_text(validated, "external_source").map(str::to_owned),
        external_id: v_text(validated, "external_id").map(str::to_owned),
        type_id: v_uuid(validated, "type"),
        git_work_branch: v_text(validated, "git_work_branch")
            .unwrap_or("")
            .to_owned(),
        workpad: String::new(),
        created_via: v_text(validated, "created_via").map(str::to_owned),
        assigned_pod_id: match validated.get("assigned_pod_id") {
            Some(Validated::Pod(pod)) => Some(pod.id),
            _ => None,
        },
        agent_executor: v_text(validated, "agent_executor").map(str::to_owned),
    };
    Ok((
        row,
        state_group,
        v_uuid_list(validated, "assignee_ids"),
        v_uuid_list(validated, "label_ids"),
    ))
}

/// `super().update(instance, validated_data)`: every present key applies
/// (`Null` clears); absent keys keep the row. `description_stripped`
/// input is skipped — `save()` recomputes it unconditionally. Returns the
/// state decision for the save legs.
fn apply_validated_to_existing(
    row: &mut IssueSaveRow,
    validated: &BTreeMap<&'static str, Validated>,
    filtered: &ValidatedAttrs,
) -> Result<(bool, Option<Uuid>, Option<String>), Denial> {
    if let Some(name) = v_text(validated, "name") {
        row.name = name.to_owned();
    }
    if validated.contains_key("point") {
        row.point = match validated.get("point") {
            Some(Validated::Int(point)) => Some(*point as i32),
            _ => None,
        };
    }
    if validated.contains_key("estimate_point") {
        row.estimate_point_id = v_uuid(validated, "estimate_point");
    }
    if validated.contains_key("parent") || validated.contains_key("parent_id") {
        row.parent_id = v_parent(validated);
    }
    if validated.contains_key("type") {
        row.type_id = v_uuid(validated, "type");
    }
    if validated.contains_key("assigned_pod_id") {
        row.assigned_pod_id = match validated.get("assigned_pod_id") {
            Some(Validated::Pod(pod)) => Some(pod.id),
            _ => None,
        };
    }
    if validated.contains_key("agent_executor") {
        row.agent_executor = v_text(validated, "agent_executor").map(str::to_owned);
    }
    if let Some(priority) = v_text(validated, "priority") {
        row.priority = priority.to_owned();
    }
    if validated.contains_key("complexity_score") {
        if let Some(Validated::Int(score)) = validated.get("complexity_score") {
            row.complexity_score = *score as i32;
        }
    }
    if validated.contains_key("start_date") {
        row.start_date = match validated.get("start_date") {
            Some(Validated::Date(date)) => Some(*date),
            _ => None,
        };
    }
    if validated.contains_key("target_date") {
        row.target_date = match validated.get("target_date") {
            Some(Validated::Date(date)) => Some(*date),
            _ => None,
        };
    }
    if validated.contains_key("description_json") {
        if let Some(Validated::Json(value)) = validated.get("description_json") {
            row.description_json = value.clone();
        }
    }
    if validated.contains_key("description_html") {
        row.description_html = filtered
            .description_html
            .clone()
            .or_else(|| v_text(validated, "description_html").map(str::to_owned))
            .unwrap_or_default();
    }
    // `sequence_id` rejects null at the field pass, so a present key is
    // always an `Int`; an out-of-`i32` value 500s like Django's `DataError`.
    if let Some(Validated::Int(sequence)) = validated.get("sequence_id") {
        row.sequence_id = i32::try_from(*sequence).map_err(|_| Denial::ServerError)?;
    }
    if validated.contains_key("sort_order") {
        if let Some(Validated::Float(order)) = validated.get("sort_order") {
            row.sort_order = *order;
        }
    }
    if validated.contains_key("completed_at") {
        row.completed_at = match validated.get("completed_at") {
            Some(Validated::DateTime(moment)) => Some(*moment),
            _ => None,
        };
    }
    if validated.contains_key("archived_at") {
        row.archived_at = match validated.get("archived_at") {
            Some(Validated::Date(date)) => Some(*date),
            _ => None,
        };
    }
    if validated.contains_key("deleted_at") {
        row.deleted_at = match validated.get("deleted_at") {
            Some(Validated::DateTime(moment)) => Some(*moment),
            _ => None,
        };
    }
    if let Some(Validated::Bool(draft)) = validated.get("is_draft") {
        row.is_draft = *draft;
    }
    if validated.contains_key("external_source") {
        row.external_source = v_text(validated, "external_source").map(str::to_owned);
    }
    if validated.contains_key("external_id") {
        row.external_id = v_text(validated, "external_id").map(str::to_owned);
    }
    if let Some(branch) = v_text(validated, "git_work_branch") {
        row.git_work_branch = branch.to_owned();
    }
    if validated.contains_key("created_via") {
        row.created_via = v_text(validated, "created_via").map(str::to_owned);
    }
    // `state` applies through the save legs (default assignment when the
    // final id is `None`), not here.
    let (touched, state_id, group) = v_state(validated);
    if touched {
        row.state_id = state_id;
    }
    Ok((touched, row.state_id, group))
}

// ---------------------------------------------------------------------------
// Create response: the 26-key `.values()` dict
// ---------------------------------------------------------------------------

/// The create post-query (`base.py:429-461`): the `issue_objects` scope +
/// `apply_annotations` + grouper `module/label/assignee_ids`, `.values()`
/// over the 26 keys, `.first()`. A miss (draft/archived/deleted create)
/// answers `None` and the handler 500s.
async fn fetch_create_response_row(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, Denial> {
    let link = link_count_select(false);
    let attachment = attachment_count_select(false);
    let sub = sub_issues_count_select(false);
    let sql = format!(
        "SELECT DISTINCT issue.id, issue.name, issue.state_id, issue.sort_order, issue.completed_at, \
         issue.estimate_point_id AS estimate_point, issue.priority, issue.start_date, \
         issue.target_date, issue.sequence_id, issue.project_id, issue.parent_id, \
         issue.created_at, issue.updated_at, issue.created_by_id AS created_by, \
         issue.updated_by_id AS updated_by, issue.is_draft, issue.archived_at, issue.deleted_at, \
         {CYCLE_ID_SELECT}, {link}, {attachment}, {sub}, {ASSIGNEE_IDS_PLAIN_SELECT}, \
         {LABEL_IDS_SELECT}, {MODULE_IDS_SELECT} \
         FROM issues AS issue \
         JOIN workspaces ON workspaces.id = issue.workspace_id \
         LEFT JOIN states AS state ON state.id = issue.state_id \
         JOIN projects AS project ON project.id = issue.project_id \
         WHERE issue.deleted_at IS NULL AND issue.id = $1 \
         AND issue.project_id = $2 AND workspaces.slug = $3 {ISSUE_OBJECTS_SCOPE} \
         ORDER BY issue.created_at DESC LIMIT 1"
    );
    sqlx::query(&sql)
        .bind(issue_id)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)
}

/// Render one `.values()` row in [`CREATE_RESPONSE_FIELDS`] wire order:
/// UUIDs as strings, `created_at`/`updated_at` in the actor's timezone
/// (the explicit `user_timezone_converter`), every other datetime UTC,
/// dates bare — exactly DRF's `JSONEncoder` over the dict.
fn render_create_response_body(
    row: &sqlx::postgres::PgRow,
    timezone: &Tz,
) -> Result<String, Denial> {
    let get = |name: &str| -> Result<String, Denial> {
        fn json(value: &impl serde::Serialize) -> Result<String, Denial> {
            serde_json::to_string(value).map_err(|_| Denial::ServerError)
        }
        match name {
            "id" => {
                let id: Uuid = row.try_get("id").map_err(|_| Denial::ServerError)?;
                json(&id.to_string())
            }
            "name" => {
                let name: String = row.try_get("name").map_err(|_| Denial::ServerError)?;
                json(&name)
            }
            "state_id" => {
                let id: Option<Uuid> = row.try_get("state_id").map_err(|_| Denial::ServerError)?;
                json(&id.map(|id| id.to_string()))
            }
            "sort_order" => {
                let order: f64 = row.try_get("sort_order").map_err(|_| Denial::ServerError)?;
                json(&order)
            }
            "completed_at" => {
                let moment: Option<DateTime<Utc>> = row
                    .try_get("completed_at")
                    .map_err(|_| Denial::ServerError)?;
                json(&moment.map(serialize_drf_datetime))
            }
            "estimate_point" => {
                let id: Option<Uuid> = row
                    .try_get("estimate_point")
                    .map_err(|_| Denial::ServerError)?;
                json(&id.map(|id| id.to_string()))
            }
            "priority" => {
                let priority: String = row.try_get("priority").map_err(|_| Denial::ServerError)?;
                json(&priority)
            }
            "start_date" | "target_date" => {
                let date: Option<NaiveDate> = row.try_get(name).map_err(|_| Denial::ServerError)?;
                json(&date.map(|date| date.to_string()))
            }
            "sequence_id" => {
                let sequence: i32 = row
                    .try_get("sequence_id")
                    .map_err(|_| Denial::ServerError)?;
                json(&sequence)
            }
            "project_id" => {
                let id: Uuid = row.try_get("project_id").map_err(|_| Denial::ServerError)?;
                json(&id.to_string())
            }
            "parent_id" => {
                let id: Option<Uuid> = row.try_get("parent_id").map_err(|_| Denial::ServerError)?;
                json(&id.map(|id| id.to_string()))
            }
            "created_at" | "updated_at" => {
                let moment: DateTime<Utc> = row.try_get(name).map_err(|_| Denial::ServerError)?;
                json(&py_iso_in(&moment, timezone))
            }
            "created_by" | "updated_by" => {
                let id: Option<Uuid> = row.try_get(name).map_err(|_| Denial::ServerError)?;
                json(&id.map(|id| id.to_string()))
            }
            "is_draft" => {
                let draft: bool = row.try_get("is_draft").map_err(|_| Denial::ServerError)?;
                json(&draft)
            }
            "archived_at" => {
                let date: Option<NaiveDate> = row
                    .try_get("archived_at")
                    .map_err(|_| Denial::ServerError)?;
                json(&date.map(|date| date.to_string()))
            }
            "deleted_at" => {
                let moment: Option<DateTime<Utc>> =
                    row.try_get("deleted_at").map_err(|_| Denial::ServerError)?;
                json(&moment.map(serialize_drf_datetime))
            }
            "cycle_id" => {
                let id: Option<Uuid> = row.try_get("cycle_id").map_err(|_| Denial::ServerError)?;
                json(&id.map(|id| id.to_string()))
            }
            "link_count" | "attachment_count" | "sub_issues_count" => {
                let count: Option<i64> = row.try_get(name).map_err(|_| Denial::ServerError)?;
                json(&count)
            }
            "assignee_ids" | "label_ids" | "module_ids" => {
                let ids: Vec<Uuid> = row.try_get(name).map_err(|_| Denial::ServerError)?;
                let rendered: Vec<String> = ids.iter().map(ToString::to_string).collect();
                json(&rendered)
            }
            _ => Err(Denial::ServerError),
        }
    };
    let mut out = String::from("{");
    for (index, field) in CREATE_RESPONSE_FIELDS.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let key = serde_json::to_string(field).map_err(|_| Denial::ServerError)?;
        out.push_str(&key);
        out.push(':');
        out.push_str(&get(field)?);
    }
    out.push('}');
    Ok(out)
}

// ---------------------------------------------------------------------------
// PUT representation: `IssueCreateSerializer(...).data`
// ---------------------------------------------------------------------------

/// Fresh m2m members for the PUT body, in M2M-manager order (target
/// `-created_at`): `serializer.data` renders the links off the saved
/// instance. The through table carries NO soft-delete filter — the m2m
/// manager joins it raw (verified from the generated SQL), so replaced
/// links still list (even twice after a delete + re-add); only the target
/// side (`labels.deleted_at`) filters.
async fn fetch_put_m2m(
    pool: &sqlx::PgPool,
    issue_id: &Uuid,
) -> Result<(Vec<Uuid>, Vec<Uuid>), Denial> {
    let assignees: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT u.id FROM users AS u JOIN issue_assignees AS ia ON ia.assignee_id = u.id \
         WHERE ia.issue_id = $1 ORDER BY u.created_at DESC",
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let labels: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT l.id FROM labels AS l JOIN issue_labels AS il ON il.label_id = l.id \
         WHERE il.issue_id = $1 AND l.deleted_at IS NULL ORDER BY l.created_at DESC",
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok((
        assignees.into_iter().map(|row| row.0).collect(),
        labels.into_iter().map(|row| row.0).collect(),
    ))
}

/// Render `serializer.data` after a PUT save: the 41-key create
/// representation off the in-memory row (no re-read) plus the fresh m2m
/// members, datetimes in DRF `Z` form, `assignee_ids`/`label_ids` echoed
/// from the raw input (falsy input echoes `[]`).
async fn render_put_body(
    pool: &sqlx::PgPool,
    row: &IssueSaveRow,
    timezone: &Tz,
    initial_assignees: Option<&Value>,
    initial_labels: Option<&Value>,
) -> Result<String, Denial> {
    let (assignees, labels) = fetch_put_m2m(pool, &row.id).await?;
    let id = row.id.to_string();
    let project = row.project_id.to_string();
    let workspace = row.workspace_id.to_string();
    let state = row.state_id.map(|value| value.to_string());
    let state_id = row.state_id.map(|value| value.to_string());
    let parent = row.parent_id.map(|value| value.to_string());
    let parent_id = row.parent_id.map(|value| value.to_string());
    let estimate_point = row.estimate_point_id.map(|value| value.to_string());
    let issue_type = row.type_id.map(|value| value.to_string());
    let assigned_pod_id = row.assigned_pod_id.map(|value| value.to_string());
    // `DateTimeField` output like the detail body: request-user zone.
    let created_at = render_datetime_in(&row.created_at, timezone);
    let updated_at = render_datetime_in(&row.updated_at, timezone);
    let deleted_at = row
        .deleted_at
        .map(|moment| render_datetime_in(&moment, timezone));
    let completed_at = row
        .completed_at
        .map(|moment| render_datetime_in(&moment, timezone));
    let archived_at = row.archived_at.map(|date| date.to_string());
    let start_date = row.start_date.map(|date| date.to_string());
    let target_date = row.target_date.map(|date| date.to_string());
    let created_by = row.created_by_id.map(|value| value.to_string());
    let updated_by = row.updated_by_id.map(|value| value.to_string());
    let assignee_strs: Vec<String> = assignees.iter().map(ToString::to_string).collect();
    let label_strs: Vec<String> = labels.iter().map(ToString::to_string).collect();
    let assignee_refs: Vec<&str> = assignee_strs.iter().map(String::as_str).collect();
    let label_refs: Vec<&str> = label_strs.iter().map(String::as_str).collect();
    let create_row = IssueCreateRow {
        id: id.as_str(),
        project: project.as_str(),
        workspace: workspace.as_str(),
        project_id: project.as_str(),
        workspace_id: workspace.as_str(),
        state: state.as_deref(),
        state_id: state_id.as_deref(),
        estimate_point: estimate_point.as_deref(),
        parent: parent.as_deref(),
        parent_id: parent_id.as_deref(),
        issue_type: issue_type.as_deref(),
        assigned_pod_id: assigned_pod_id.as_deref(),
        assignees: &assignee_refs,
        labels: &label_refs,
        name: row.name.as_str(),
        description_json: &row.description_json,
        description_html: row.description_html.as_str(),
        description_stripped: row.description_stripped.as_deref(),
        description_binary: row.description_binary.as_deref(),
        priority: row.priority.as_str(),
        complexity_score: row.complexity_score,
        start_date: start_date.as_deref(),
        target_date: target_date.as_deref(),
        sequence_id: row.sequence_id,
        sort_order: row.sort_order,
        completed_at: completed_at.as_deref(),
        archived_at: archived_at.as_deref(),
        is_draft: row.is_draft,
        external_source: row.external_source.as_deref(),
        external_id: row.external_id.as_deref(),
        git_work_branch: row.git_work_branch.as_str(),
        created_via: row.created_via.as_deref(),
        agent_executor: row.agent_executor.as_deref(),
        point: row.point,
        deleted_at: deleted_at.as_deref(),
        created_at: created_at.as_str(),
        updated_at: updated_at.as_str(),
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
    };
    let view = issue_create_to_representation(&create_row, initial_assignees, initial_labels);
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Shared update legs
// ---------------------------------------------------------------------------

/// The save legs for an update-shaped write (PATCH/PUT/destroy):
/// `row.state_id` is final on entry (applied or kept). A `None` id takes
/// the default assignment (`self.project` touch + default queries,
/// `completed_at` untouched); a set id recomputes `completed_at` off the
/// touched group or a fresh `self.state` read (whose miss is the
/// required-object 404). `description_stripped` always recomputes.
async fn update_state_legs(
    pool: &sqlx::PgPool,
    row: &mut IssueSaveRow,
    touched: bool,
    group: Option<String>,
) -> Result<(), Denial> {
    if row.state_id.is_none() {
        touch_save_project(pool, &row.project_id).await?;
        row.state_id = default_state_for_project(pool, &row.project_id).await?;
    } else {
        let group = match (touched, group) {
            (_, Some(group)) => group,
            (false, None) => {
                let state_id = row.state_id.ok_or(Denial::ServerError)?;
                saved_state_group(pool, &state_id)
                    .await?
                    .ok_or(Denial::NotFound)?
            }
            // The field `get()` always resolves the group of a touched state.
            (true, None) => return Err(Denial::ServerError),
        };
        if group == "completed" {
            row.completed_at = Some(utc_now_micros());
        } else {
            row.completed_at = None;
        }
    }
    row.description_stripped = sync_description_stripped(Some(&row.description_html));
    Ok(())
}

/// One update m2m leg (`serializers/issue.py:477-511`): present keys
/// soft-delete the live links, then re-insert the effective ids with
/// `ON CONFLICT DO NOTHING`; audit ids come from the PRE-SAVE instance.
async fn update_m2m_leg(
    pool: &sqlx::PgPool,
    table: &str,
    member_column: &str,
    present: bool,
    effective: &[Uuid],
    row: &IssueSaveRow,
    audit: (Option<Uuid>, Option<Uuid>),
) -> Result<(), Denial> {
    if !present {
        return Ok(());
    }
    let sql =
        format!("UPDATE {table} SET deleted_at = $1 WHERE issue_id = $2 AND deleted_at IS NULL");
    sqlx::query(&sql)
        .bind(utc_now_micros())
        .bind(row.id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    write_m2m_batches(
        pool,
        table,
        member_column,
        effective,
        &row.id,
        &row.project_id,
        &row.workspace_id,
        audit.0,
        audit.1,
        true,
    )
    .await
}

/// Split a proxied-or-read request after the UUID guards passed (mirrors
/// the sibling handlers' 8MB cap; overflow 500s).
async fn split_core_parts(
    req: axum::extract::Request,
) -> Result<(axum::http::request::Parts, Vec<u8>), Denial> {
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, 8 * 1024 * 1024)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok((parts, bytes.to_vec()))
}

fn host_settings_of(state: &AppState) -> HostSettings<'_> {
    let urls = &state.settings().urls;
    HostSettings {
        web_url: urls.web_url.as_deref(),
        app_base_url: urls.app_base_url.as_deref(),
        admin_base_url: None,
        space_base_url: None,
        admin_base_path: None,
        space_base_path: None,
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST .../issues/` (`base.py:397-485`): ADMIN/MEMBER gate (no creator
/// bypass), unscoped project fetch, serializer validation, the atomic
/// sequence/sort INSERT, m2m + default-assignee legs, the explicit
/// transition fire (always immediate), `issue_activity` ahead of the
/// post-query 500, then `model_activity` + the creating version task.
pub async fn create_issue(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = core_session(extension)?;
    let tenant = core_gate(
        &pool,
        &slug,
        &project_raw,
        &user_id,
        &GATE_CREATE,
        None,
        ProjectCheck::AnyWorkspace,
    )
    .await?;
    let (parts, bytes) = split_core_parts(req).await?;
    let data = match negotiate_input(&parts.headers, &bytes) {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    // A non-dict body fails the whole serializer (`non_field_errors`) —
    // the field loop never runs, so no `required` error joins it.
    if data.value.as_object().is_none() {
        return Ok(non_dict_response(&data.value));
    }
    let validated =
        match validate_issue_body(&pool, &data, RequestShape::Create, &tenant.timezone).await {
            Ok(validated) => validated,
            Err(failure) => return field_pass_response(failure),
        };
    let filtered = match run_validate(
        &pool,
        &state,
        &validated,
        Some(tenant.project_id),
        None,
        &tenant,
    )
    .await
    {
        Ok(filtered) => filtered,
        Err(response) => return Ok(response),
    };
    let (mut row, state_group, raw_assignees, raw_labels) =
        assemble_new_issue(&validated, &filtered, &tenant)?;
    if row.assigned_pod_id.is_none() {
        row.assigned_pod_id = default_pod_for_project(&pool, &tenant.project_id).await?;
    }
    if row.state_id.is_none() {
        // The `self.project` touch runs inside the atomic block on the
        // state-given leg (the lock key); hoisted here — a `SELECT`'s
        // transaction membership is unobservable.
        touch_save_project(&pool, &tenant.project_id).await?;
        row.state_id = default_state_for_project(&pool, &tenant.project_id).await?;
    } else {
        let group = match state_group {
            Some(group) => group,
            // Unreachable: a given state carries its group from the field
            // `get()`; fall back to the `self.state` read regardless.
            None => {
                let state_id = row.state_id.ok_or(Denial::ServerError)?;
                saved_state_group(&pool, &state_id)
                    .await?
                    .ok_or(Denial::ServerError)?
            }
        };
        if group == "completed" {
            row.completed_at = Some(utc_now_micros());
        } else {
            row.completed_at = None;
        }
    }
    let mut tx = pool.begin().await.map_err(|_| Denial::ServerError)?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(advisory_lock_key(&tenant.project_id))
        .execute(&mut *tx)
        .await
        .map_err(|_| Denial::ServerError)?;
    let max_sequence: (Option<i64>,) = sqlx::query_as(
        "SELECT MAX(sequence) FROM issue_sequences WHERE project_id = $1 AND deleted_at IS NULL",
    )
    .bind(tenant.project_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.sequence_id = i32::try_from(max_sequence.0.map(|max| max + 1).unwrap_or(1))
        .map_err(|_| Denial::ServerError)?;
    let max_sort: (Option<f64>,) = match row.state_id {
        Some(state_id) => sqlx::query_as(
            "SELECT MAX(sort_order) FROM issues \
                 WHERE project_id = $1 AND state_id = $2 AND deleted_at IS NULL",
        )
        .bind(tenant.project_id)
        .bind(state_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| Denial::ServerError)?,
        None => sqlx::query_as(
            "SELECT MAX(sort_order) FROM issues \
             WHERE project_id = $1 AND state_id IS NULL AND deleted_at IS NULL",
        )
        .bind(tenant.project_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| Denial::ServerError)?,
    };
    if let Some(largest) = max_sort.0 {
        row.sort_order = largest + 10000.0;
    }
    row.description_stripped = sync_description_stripped(Some(&row.description_html));
    insert_issue_row(&mut tx, &row).await?;
    sqlx::query(
        "INSERT INTO issue_sequences (id, created_at, updated_at, created_by_id, updated_by_id, \
         deleted_at, project_id, workspace_id, issue_id, sequence, deleted) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(Uuid::new_v4())
    .bind(utc_now_micros())
    .bind(utc_now_micros())
    .bind(Some(tenant.user_id))
    .bind(None::<Uuid>)
    .bind(None::<DateTime<Utc>>)
    .bind(tenant.project_id)
    .bind(tenant.workspace_id)
    .bind(row.id)
    .bind(i64::from(row.sequence_id))
    .bind(false)
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        if is_integrity_violation(&error) {
            Denial::BadError("The payload is not valid".to_owned())
        } else {
            Denial::ServerError
        }
    })?;
    tx.commit().await.map_err(|_| Denial::ServerError)?;
    // The m2m legs run post-commit in autocommit (the `except
    // IntegrityError: pass` around each `bulk_create`).
    let assignees = filtered
        .assignee_ids
        .clone()
        .or(raw_assignees)
        .unwrap_or_default();
    if assignees.is_empty() {
        if let Some(default) = tenant.default_assignee_id {
            write_default_assignee(
                &pool,
                &default,
                &row.id,
                &tenant.project_id,
                &tenant.workspace_id,
                row.created_by_id,
            )
            .await?;
        }
    } else {
        write_m2m_batches(
            &pool,
            "issue_assignees",
            "assignee_id",
            &assignees,
            &row.id,
            &tenant.project_id,
            &tenant.workspace_id,
            row.created_by_id,
            None,
            false,
        )
        .await?;
    }
    let labels = filtered
        .label_ids
        .clone()
        .or(raw_labels)
        .unwrap_or_default();
    if !labels.is_empty() {
        write_m2m_batches(
            &pool,
            "issue_labels",
            "label_id",
            &labels,
            &row.id,
            &tenant.project_id,
            &tenant.workspace_id,
            row.created_by_id,
            None,
            false,
        )
        .await?;
    }
    let mut seam = CoreSignalSeam { pool: &pool };
    let prev = capture_prior_state(&mut seam, None)
        .await
        .map_err(|_| Denial::ServerError)?;
    fire_after_save(
        &pool,
        row.id,
        prev,
        row.state_id,
        true,
        Some(user_id),
        utc_now_micros(),
    )
    .await?;
    // `base_host` raises past the save (the issue + no jobs persist on a
    // misconfigured host), so the origin resolves here, not up front.
    let origin = request_origin(&state)?;
    let requested = to_spaced_json(&data.value)?;
    enqueue_issue_activity(
        &pool,
        "issue.activity.created",
        Value::String(requested.clone()),
        &user_id,
        &row.id,
        &tenant.project_id,
        Value::Null,
        &origin,
        None,
    )
    .await;
    let Some(post) = fetch_create_response_row(&pool, &slug, &tenant.project_id, &row.id).await?
    else {
        // `user_timezone_converter(None)` → `TypeError` → the generic 500,
        // with the row + `issue_activity` already persisted.
        return Err(Denial::ServerError);
    };
    enqueue_model_activity(
        &pool,
        &row.id,
        data.value.clone(),
        Value::Null,
        &user_id,
        &slug,
        &origin,
    )
    .await;
    enqueue_version_task(&pool, &requested, &row.id, &user_id, Some(true)).await;
    Ok(json_created(render_create_response_body(
        &post,
        &tenant.timezone,
    )?))
}

/// `PATCH .../issues/<pk>/` (`base.py:621-721`): ADMIN/MEMBER/creator
/// gate, the `issue_objects` annotated lookup, the pre-update
/// `current_instance` detail dump, partial validation, explicit capture +
/// save + fire (honoring `X-Pi-Dash-Skip-Immediate-Dispatch`), the three
/// task enqueues (skipped for description-only migrations skips), 204.
pub async fn partial_update_issue(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    // Non-UUID tails never match Django's `<uuid:pk>` converter: proxy.
    let Ok(pk) = pk_raw.parse::<Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = core_session(extension)?;
    let mut tenant = core_gate(
        &pool,
        &slug,
        &project_raw,
        &user_id,
        &GATE_PATCH,
        Some(pk),
        ProjectCheck::None,
    )
    .await?;
    let (parts, bytes) = split_core_parts(req).await?;
    let mut data = match negotiate_input(&parts.headers, &bytes) {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    // `request.data.pop(...)` runs before the lookup: on a non-dict body
    // the pop itself raises (`TypeError`/`AttributeError`) → Django's 500,
    // regardless of whether the issue exists.
    if data.value.as_object().is_none() {
        return Err(Denial::ServerError);
    }
    // `request.data.pop("skip_activity", False)` mutates the parsed body —
    // both dumps below exclude the key.
    let skip_activity = data.pop_skip_activity();
    let is_description_update = data.get_is_set("description_html");
    let Some(looked_up) =
        fetch_annotated_issue(&pool, &slug, &tenant.project_id, &pk, &user_id, true).await?
    else {
        return Ok(not_found_detail(
            ISSUE_NOT_FOUND_BODY,
            StatusCode::NOT_FOUND,
        ));
    };
    tenant.workspace_id = looked_up.save.workspace_id;
    let current_instance =
        render_detail_body(&pool, &looked_up, &tenant.timezone, false, false, true).await?;
    let requested = to_spaced_json(&data.value)?;
    let validated =
        match validate_issue_body(&pool, &data, RequestShape::Patch, &tenant.timezone).await {
            Ok(validated) => validated,
            Err(failure) => return field_pass_response(failure),
        };
    let instance = looked_up.write_instance();
    let filtered = match run_validate(
        &pool,
        &state,
        &validated,
        Some(tenant.project_id),
        Some(&instance),
        &tenant,
    )
    .await
    {
        Ok(filtered) => filtered,
        Err(response) => return Ok(response),
    };
    let mut seam = CoreSignalSeam { pool: &pool };
    let prev = capture_prior_state(&mut seam, Some(pk))
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut row = looked_up.save;
    let audit = (row.created_by_id, row.updated_by_id);
    let (touched, _, group) = apply_validated_to_existing(&mut row, &validated, &filtered)?;
    // The m2m legs run ahead of the row write (`update()` pops them first).
    let (assignee_present, assignees) =
        v_m2m_effective(filtered.assignee_ids.clone(), &validated, "assignee_ids");
    update_m2m_leg(
        &pool,
        "issue_assignees",
        "assignee_id",
        assignee_present,
        &assignees,
        &row,
        audit,
    )
    .await?;
    let (label_present, labels) =
        v_m2m_effective(filtered.label_ids.clone(), &validated, "label_ids");
    update_m2m_leg(
        &pool,
        "issue_labels",
        "label_id",
        label_present,
        &labels,
        &row,
        audit,
    )
    .await?;
    update_state_legs(&pool, &mut row, touched, group).await?;
    // `update()` assigns `updated_at`, then `save()`'s `auto_now`
    // overwrites it — the net effect is one save-time stamp.
    row.updated_by_id = Some(user_id);
    row.updated_at = utc_now_micros();
    update_issue_row(&pool, &row).await?;
    let dispatch_immediate = !skip_immediate_dispatch(&parts.headers);
    fire_after_save(
        &pool,
        pk,
        prev,
        row.state_id,
        dispatch_immediate,
        Some(user_id),
        utc_now_micros(),
    )
    .await?;
    if !(skip_activity && is_description_update) {
        let origin = request_origin(&state)?;
        enqueue_issue_activity(
            &pool,
            "issue.activity.updated",
            Value::String(requested.clone()),
            &user_id,
            &pk,
            &tenant.project_id,
            Value::String(current_instance.clone()),
            &origin,
            None,
        )
        .await;
        enqueue_model_activity(
            &pool,
            &pk,
            data.value.clone(),
            Value::String(current_instance.clone()),
            &user_id,
            &slug,
            &origin,
        )
        .await;
        enqueue_version_task(&pool, &current_instance, &pk, &user_id, None).await;
    }
    Ok(empty_response())
}

/// `PUT .../issues/<pk>/` (DRF's default `update` — deliberately
/// undecorated, so any authenticated member including guests and
/// non-members 200s: the recorded outsider hole): full validation with a
/// project-less context (the assignee `KeyError`, ignored `label_ids` — the
/// rows are kept, there is no wipe — and state/parent/estimate `non_field`
/// arms), explicit capture + save + always-immediate fire, no task
/// enqueues, 200 + the create representation.
pub async fn put_update_issue(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let Ok(pk) = pk_raw.parse::<Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = core_session(extension)?;
    // `initial()` still runs: identifier-rewrite 404, then the user row +
    // tz activation (`TimezoneMixin` 500s ahead of the body) — but no
    // decorator and no tenant project fetch.
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let (timezone, user_active, user_bot) = core_user(&pool, &user_id).await?;
    let (parts, bytes) = split_core_parts(req).await?;
    let data = match negotiate_input(&parts.headers, &bytes) {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    // DRF's `update` looks the object up before touching the serializer.
    let Some(looked_up) = fetch_put_issue(&pool, &slug, &project_id, &pk).await? else {
        return Ok(not_found_detail(PUT_MISSING_BODY, StatusCode::NOT_FOUND));
    };
    let tenant = CoreTenant {
        project_id,
        workspace_id: looked_up.workspace_id,
        user_id,
        timezone,
        user_active,
        user_bot,
        default_assignee_id: None,
        guest_view_all_features: false,
    };
    // `get_object` already ran: a non-dict body now fails the whole
    // serializer (`non_field_errors`), same as create.
    if data.value.as_object().is_none() {
        return Ok(non_dict_response(&data.value));
    }
    let validated =
        match validate_issue_body(&pool, &data, RequestShape::Put, &tenant.timezone).await {
            Ok(validated) => validated,
            Err(failure) => return field_pass_response(failure),
        };
    let instance = looked_up.write_instance();
    let filtered =
        match run_validate(&pool, &state, &validated, None, Some(&instance), &tenant).await {
            Ok(filtered) => filtered,
            Err(response) => return Ok(response),
        };
    let mut seam = CoreSignalSeam { pool: &pool };
    let prev = capture_prior_state(&mut seam, Some(pk))
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut row = looked_up;
    let audit = (row.created_by_id, row.updated_by_id);
    let (touched, _, group) = apply_validated_to_existing(&mut row, &validated, &filtered)?;
    let (assignee_present, assignees) =
        v_m2m_effective(filtered.assignee_ids.clone(), &validated, "assignee_ids");
    update_m2m_leg(
        &pool,
        "issue_assignees",
        "assignee_id",
        assignee_present,
        &assignees,
        &row,
        audit,
    )
    .await?;
    let (label_present, labels) =
        v_m2m_effective(filtered.label_ids.clone(), &validated, "label_ids");
    update_m2m_leg(
        &pool,
        "issue_labels",
        "label_id",
        label_present,
        &labels,
        &row,
        audit,
    )
    .await?;
    update_state_legs(&pool, &mut row, touched, group).await?;
    row.updated_by_id = Some(user_id);
    row.updated_at = utc_now_micros();
    update_issue_row(&pool, &row).await?;
    fire_after_save(
        &pool,
        pk,
        prev,
        row.state_id,
        true,
        Some(user_id),
        utc_now_micros(),
    )
    .await?;
    let initial = data.value.as_object();
    let body = render_put_body(
        &pool,
        &row,
        &tenant.timezone,
        initial.and_then(|object| object.get("assignee_ids")),
        initial.and_then(|object| object.get("label_ids")),
    )
    .await?;
    Ok(json_response(body))
}

/// `DELETE .../issues/<pk>/` (`base.py:722-755`): ADMIN/creator gate, the
/// live-only lookup, the git-sync 409 guard, then the soft delete (a
/// `save()` — the state legs run — plus the `soft_delete_related_objects`
/// forward), the hard recent-visit delete, and the unsubscribed
/// `deleted` activity, 204.
pub async fn destroy_issue(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let Ok(pk) = pk_raw.parse::<Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = core_session(extension)?;
    let tenant = core_gate(
        &pool,
        &slug,
        &project_raw,
        &user_id,
        &GATE_DESTROY,
        Some(pk),
        ProjectCheck::None,
    )
    .await?;
    let Some(mut row) = fetch_destroy_issue(&pool, &slug, &tenant.project_id, &pk).await? else {
        return Ok(not_found_detail(NOT_FOUND_BODY, StatusCode::NOT_FOUND));
    };
    // `GitIssueSync...exists() or GithubIssueSync...exists()` — short-circuit.
    let synced = git_issue_synced(&pool, &pk)
        .await
        .map_err(|_| Denial::ServerError)?
        || github_issue_synced(&pool, &pk)
            .await
            .map_err(|_| Denial::ServerError)?;
    if synced {
        return Ok(not_found_detail(DESTROY_SYNCED_BODY, StatusCode::CONFLICT));
    }
    let mut seam = CoreSignalSeam { pool: &pool };
    let prev = capture_prior_state(&mut seam, Some(pk))
        .await
        .map_err(|_| Denial::ServerError)?;
    // `issue.delete()`: `deleted_at` now, then the full `save()` (the
    // state legs run — a triage/deleted state 404s here — plus stripped
    // + audit), then the related-objects forward.
    row.deleted_at = Some(utc_now_micros());
    update_state_legs(&pool, &mut row, false, None).await?;
    row.updated_by_id = Some(user_id);
    row.updated_at = utc_now_micros();
    update_issue_row(&pool, &row).await?;
    fire_after_save(
        &pool,
        pk,
        prev,
        row.state_id,
        true,
        Some(user_id),
        utc_now_micros(),
    )
    .await?;
    enqueue_soft_delete(&pool, &pk).await;
    sqlx::query(
        "DELETE FROM user_recent_visits AS visit USING workspaces AS workspace \
         WHERE visit.workspace_id = workspace.id AND visit.project_id = $1 \
         AND workspace.slug = $2 AND visit.entity_identifier = $3 AND visit.entity_name = 'issue'",
    )
    .bind(tenant.project_id)
    .bind(slug.as_str())
    .bind(pk)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let origin = request_origin(&state)?;
    let mut deleted_key = Map::new();
    deleted_key.insert("issue_id".to_owned(), Value::String(pk.to_string()));
    let requested = to_spaced_json(&deleted_key)?;
    enqueue_issue_activity(
        &pool,
        "issue.activity.deleted",
        Value::String(requested),
        &user_id,
        &pk,
        &tenant.project_id,
        Value::Object(Map::new()),
        &origin,
        Some(false),
    )
    .await;
    Ok(empty_response())
}

/// `GET .../issues/<pk>/` (`base.py:486-620`): ADMIN/MEMBER/GUEST/creator
/// gate, the slug-scoped project fetch, the live-only annotated lookup,
/// the guest-view rule, the visit enqueue (which fires even when the
/// `sub_issues` expansion 500s below it), and the 35-key detail body.
/// Unknown expansions are ignored; every other expansion is a known gap.
pub async fn retrieve_issue(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let Ok(pk) = pk_raw.parse::<Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = core_session(extension)?;
    let tenant = core_gate(
        &pool,
        &slug,
        &project_raw,
        &user_id,
        &GATE_RETRIEVE,
        Some(pk),
        ProjectCheck::InSlug,
    )
    .await?;
    let (parts, _) = split_core_parts(req).await?;
    let Some(looked_up) =
        fetch_annotated_issue(&pool, &slug, &tenant.project_id, &pk, &user_id, false).await?
    else {
        return Ok(not_found_detail(NOT_FOUND_BODY, StatusCode::NOT_FOUND));
    };
    // The guest-view rule evaluates the role-5 `EXISTS` on every call.
    let guest: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM project_members AS member \
         JOIN workspaces ON workspaces.id = member.workspace_id \
         WHERE member.member_id = $1 AND member.project_id = $2 AND workspaces.slug = $3 \
         AND member.role = 5 AND member.is_active AND member.deleted_at IS NULL LIMIT 1",
    )
    .bind(user_id)
    .bind(tenant.project_id)
    .bind(slug.as_str())
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if guest.is_some()
        && !tenant.guest_view_all_features
        && looked_up.save.created_by_id != Some(user_id)
    {
        return Ok(not_found_detail(GUEST_VIEW_BODY, StatusCode::FORBIDDEN));
    }
    enqueue_recent_visited(&pool, &slug, &pk.to_string(), &user_id, &tenant.project_id).await;
    // `Issue.sub_issues` does not exist, so the `IssueLiteSerializer`
    // expansion raises `AttributeError` → the generic 500 (the visit
    // enqueue above already fired). Ported as the bug it is.
    let expand = parts
        .uri
        .query()
        .and_then(|query| {
            query.split('&').find_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                (key == "expand").then_some(value)
            })
        })
        .unwrap_or("");
    if expand.split(',').any(|name| name == "sub_issues") {
        return Err(Denial::ServerError);
    }
    Ok(json_response(
        render_detail_body(&pool, &looked_up, &tenant.timezone, true, true, false).await?,
    ))
}

/// `DELETE .../bulk-delete-issues/` (`base.py:786-812`): ADMIN gate, the
/// lenient `issue_ids` sweep (missing/empty → required; `None`/int →
/// 500; strings iterate chars; int/bool/`None` items drop silently;
/// anything else unparseable → valid-detail 400), the `issue_objects`
/// count, the cycle/module link soft-deletes plus the issue soft-delete,
/// 200 with the count message. No signals fire (queryset updates).
pub async fn bulk_delete_issues(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = core_session(extension)?;
    let tenant = core_gate(
        &pool,
        &slug,
        &project_raw,
        &user_id,
        &GATE_BULK_DELETE,
        None,
        ProjectCheck::None,
    )
    .await?;
    let (parts, bytes) = split_core_parts(req).await?;
    let data = match negotiate_input(&parts.headers, &bytes) {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    // A non-object body has no `.get` → `AttributeError` → generic 500.
    let Some(object) = data.value.as_object() else {
        return Err(Denial::ServerError);
    };
    // `request.data.get("issue_ids", [])`, then `len()` — `None`/numbers
    // have no length → `TypeError` → generic 500.
    static EMPTY_IDS: Value = Value::Array(vec![]);
    let raw = object.get("issue_ids").unwrap_or(&EMPTY_IDS);
    let survivors = match raw {
        Value::Null => return Err(Denial::ServerError),
        Value::Bool(_) | Value::Number(_) => return Err(Denial::ServerError),
        Value::Array(_) | Value::Object(_) | Value::String(_) => {
            let len = match raw {
                Value::Array(items) => items.len(),
                Value::Object(map) => map.len(),
                // Python `len()` counts chars, not bytes.
                Value::String(text) => text.chars().count(),
                _ => 0,
            };
            if len == 0 {
                return Ok(json_bad_request(ISSUE_IDS_REQUIRED_BODY));
            }
            sweep_bulk_ids(raw)?
        }
    };
    let total = count_bulk_issues(&pool, &slug, &tenant.project_id, &survivors).await?;
    if !survivors.is_empty() {
        let placeholders = (0..survivors.len())
            .map(|index| format!("${}", index + 2))
            .collect::<Vec<_>>()
            .join(", ");
        // Each `.delete()` evaluates its own `now()`.
        for table in ["cycle_issues", "module_issues"] {
            let sql = format!(
                "UPDATE {table} AS link SET deleted_at = $1 WHERE link.deleted_at IS NULL \
                 AND link.issue_id IN ({placeholders})"
            );
            let mut query = sqlx::query(&sql).bind(utc_now_micros());
            for id in &survivors {
                query = query.bind(id);
            }
            query
                .execute(&pool)
                .await
                .map_err(|_| Denial::ServerError)?;
        }
        // The issues leg reuses the `issue_objects` scope (it soft-deletes
        // only in-scope rows; out-of-scope ids still count as misses).
        // The triage span is a null-safe `NOT EXISTS` (Django's
        // `LEFT JOIN` scope keeps null-state rows; the spanned manager
        // only joins live states). Project + workspace scoping is load
        // bearing: without it a foreign id in the list would soft-delete
        // another project's issue, which Django's filtered queryset never
        // touches.
        let project_bind = survivors.len() + 2;
        let slug_bind = survivors.len() + 3;
        // Comma-join, not JOIN..ON: Postgres forbids referencing the
        // UPDATE target from a FROM-clause ON expression.
        let sql = format!(
            "UPDATE issues AS issue SET deleted_at = $1 \
             FROM projects AS project, workspaces AS workspace \
             WHERE issue.deleted_at IS NULL AND issue.id IN ({placeholders}) \
             AND issue.project_id = ${project_bind} AND issue.project_id = project.id \
             AND issue.workspace_id = workspace.id AND workspace.slug = ${slug_bind} \
             AND NOT EXISTS (SELECT 1 FROM states AS state \
             WHERE state.id = issue.state_id AND state.deleted_at IS NULL \
             AND state.\"group\" = 'triage') \
             AND issue.archived_at IS NULL AND project.archived_at IS NULL \
             AND issue.is_draft = FALSE"
        );
        let mut query = sqlx::query(&sql).bind(utc_now_micros());
        for id in &survivors {
            query = query.bind(id);
        }
        query = query.bind(tenant.project_id).bind(slug.as_str());
        query
            .execute(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    }
    let message = serde_json::to_string(&format!("{total} issues were deleted"))
        .map_err(|_| Denial::ServerError)?;
    Ok(json_response(format!("{{\"message\":{message}}}")))
}

/// The lenient `pk__in` sweep: strings must parse as UUIDs (chars of a
/// string value, keys of an object value), int/bool/`None` items drop
/// silently, floats/containers 400 — verified live, item by item.
fn sweep_bulk_ids(raw: &Value) -> Result<Vec<Uuid>, Denial> {
    let invalid = || Denial::BadError("Please provide valid detail".to_owned());
    let parse = |text: &str| Uuid::parse_str(text).map_err(|_| invalid());
    match raw {
        Value::String(text) => text.chars().map(|char| parse(&char.to_string())).collect(),
        Value::Object(map) => map.keys().map(|key| parse(key)).collect(),
        Value::Array(items) => {
            let mut survivors = Vec::new();
            for item in items {
                match item {
                    Value::String(text) => survivors.push(parse(text)?),
                    Value::Number(_) | Value::Bool(_) | Value::Null => {}
                    _ => return Err(invalid()),
                }
            }
            Ok(survivors)
        }
        _ => Err(Denial::ServerError),
    }
}

/// `len(issues)`: the `issue_objects`-scoped count behind the message.
async fn count_bulk_issues(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    survivors: &[Uuid],
) -> Result<i64, Denial> {
    let mut sql = format!(
        "SELECT issue.id FROM issues AS issue \
         JOIN workspaces ON workspaces.id = issue.workspace_id \
         LEFT JOIN states AS state ON state.id = issue.state_id \
         JOIN projects AS project ON project.id = issue.project_id \
         WHERE issue.deleted_at IS NULL AND issue.project_id = $1 \
         AND workspaces.slug = $2 {ISSUE_OBJECTS_SCOPE}"
    );
    if survivors.is_empty() {
        sql.push_str(" AND FALSE");
        let rows: Vec<(Uuid,)> = sqlx::query_as(&sql)
            .bind(project_id)
            .bind(slug)
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        return Ok(rows.len() as i64);
    }
    let placeholders = (0..survivors.len())
        .map(|index| format!("${}", index + 3))
        .collect::<Vec<_>>()
        .join(", ");
    sql.push_str(&format!(" AND issue.id IN ({placeholders})"));
    let mut query = sqlx::query_as::<_, (Uuid,)>(&sql)
        .bind(project_id)
        .bind(slug);
    for id in survivors {
        query = query.bind(id);
    }
    let rows: Vec<(Uuid,)> = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(rows.len() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(literal: &str) -> Value {
        serde_json::from_str(literal).expect("number")
    }

    #[test]
    fn int_literals_spell_exactly() {
        let huge = num("99999999999999999999999");
        let number = huge.as_number().expect("number");
        assert_eq!(
            int_literal(number).as_deref(),
            Some("99999999999999999999999")
        );
        assert_eq!(json_number_str(number), "99999999999999999999999");
        // In-range magnitudes format numerically (`-0` → `"0"`).
        assert_eq!(json_number_str(num("-0").as_number().unwrap()), "0");
        assert_eq!(json_number_str(num("5").as_number().unwrap()), "5");
        // Exponent shapes are floats (`str(1000.0)`), never int literals.
        assert!(int_literal(num("1e3").as_number().unwrap()).is_none());
        assert_eq!(json_number_str(num("1e3").as_number().unwrap()), "1000.0");
        assert_eq!(json_type_name(&num("99999999999999999999999")), "int");
        assert_eq!(json_type_name(&num("1e3")), "float");
    }

    #[test]
    fn huge_integers_report_min_max_not_invalid() {
        assert!(matches!(
            parse_integer(&num("99999999999999999999999")),
            Ok(IntegerValue::TooBig)
        ));
        assert!(matches!(
            parse_integer(&num("-99999999999999999999999")),
            Ok(IntegerValue::TooSmall)
        ));
        assert!(matches!(
            parse_integer(&num("1e3")),
            Ok(IntegerValue::Int(1000))
        ));
    }

    #[test]
    fn char_fields_reject_nul_and_stringify_exponents() {
        assert_eq!(
            parse_char(&Value::String("a\0b".to_owned()), None),
            Err(vec![MSG_NULL_CHARACTERS.to_owned()])
        );
        assert_eq!(parse_char(&num("1e3"), None).as_deref(), Ok("1000.0"));
        assert_eq!(parse_char(&num("123"), None).as_deref(), Ok("123"));
    }

    #[test]
    fn py_repr_picks_quotes_like_cpython() {
        assert_eq!(py_repr_string("it's"), "\"it's\"");
        assert_eq!(py_repr_string("say \"hi\""), "'say \"hi\"'");
        assert_eq!(py_repr_string("both'and\"q"), "'both\\'and\"q'");
        assert_eq!(py_repr_string("plain"), "'plain'");
    }

    #[test]
    fn indexed_item_errors_order_numerically() {
        // Eleven items, indices 2 and 10 invalid: `"10"` must not sort
        // before `"2"` (Django dicts follow loop order).
        let items: Vec<Value> = (0..11)
            .map(|index| {
                if index == 2 || index == 10 {
                    Value::String("nope".to_owned())
                } else {
                    Value::String("00000000-0000-0000-0000-000000000000".to_owned())
                }
            })
            .collect();
        let ListOutcome::Indexed(errors) = parse_uuid_list(&Value::Array(items)) else {
            panic!("expected indexed errors");
        };
        let keys: Vec<usize> = errors.keys().copied().collect();
        assert_eq!(keys, vec![2, 10]);
        let rendered = serde_json::to_string(&indexed_errors(errors)).expect("render");
        assert!(rendered.find("\"2\"").unwrap() < rendered.find("\"10\"").unwrap());
    }

    #[test]
    fn short_and_multibyte_dates_reject_without_panicking() {
        // Short basic-week inputs panicked the `[5..7]` slice.
        assert_eq!(parse_drf_date("2024W"), None);
        assert_eq!(parse_drf_date("2024W1"), None);
        // Multibyte bytes straddling a cut panicked the slices.
        assert_eq!(parse_drf_date("2024-W5ä"), None);
        assert_eq!(split_date_time("2024-01-1äT00:00"), None);
        assert_eq!(parse_iso_time("1ä3"), None);
        // Valid shapes still parse, including multibyte separators
        // (`fromisoformat` accepts any single char there).
        assert!(parse_drf_date("2024W05").is_some());
        assert!(parse_drf_date("2024W053").is_some());
        assert!(parse_drf_date("2024-W05-3").is_some());
        assert!(split_date_time("20240101é00:00").is_some());
        assert!(parse_iso_time("1234").is_some());
    }

    #[test]
    fn extended_weeks_parse_and_plus_fields_reject() {
        // `YYYY-Www[-D]` is valid `fromisoformat` (the year half keeps its
        // dash before the `W` split).
        assert!(parse_drf_date("2024-W05").is_some());
        assert!(parse_drf_date("2024-W05-3").is_some());
        // Short years, day 0 (no underflow), and `+`-signed fields reject.
        assert_eq!(parse_drf_date("24W05"), None);
        assert_eq!(parse_drf_date("2024-W05-0"), None);
        assert_eq!(parse_drf_date("2024-+1-01"), None);
        assert_eq!(parse_drf_date("2024W+1"), None);
    }

    #[test]
    fn tz_offsets_are_strict_two_digit() {
        assert_eq!(
            split_tz_suffix("2024-01-01T00:00:00+05:30:05"),
            ("2024-01-01T00:00:00", Some(5 * 3600 + 30 * 60 + 5))
        );
        // One-digit fields reject; fractions truncate.
        assert_eq!(
            split_tz_suffix("2024-01-01T00:00:00+1:00"),
            ("2024-01-01T00:00:00+1:00", None)
        );
        assert_eq!(
            split_tz_suffix("2024-01-01T00:00:00+05:30:5"),
            ("2024-01-01T00:00:00+05:30:5", None)
        );
        assert_eq!(
            split_tz_suffix("2024-01-01T00:00:00+00:00:00.5"),
            ("2024-01-01T00:00:00", Some(0))
        );
        assert_eq!(
            split_tz_suffix("2024-01-01T00:00:00+00:00:99"),
            ("2024-01-01T00:00:00", Some(99))
        );
    }

    #[test]
    fn drf_datetime_iso8601_literal_fallback() {
        // PIDASHCONV-773: DRF `to_internal_value` falls through to
        // `strptime(value, 'iso-8601')` when `parse_datetime` returns None;
        // the literal matches case-insensitively and yields naive
        // 1900-01-01 in the actor zone (probed live both backends).
        let utc: Tz = "UTC".parse().unwrap();
        for text in [
            "iso-8601", "ISO-8601", "Iso-8601", "iSo-8601", "isO-8601", "ISo-8601", "IsO-8601",
            "iSO-8601",
        ] {
            let parsed = parse_drf_datetime(text, &utc).expect(text);
            assert_eq!(parsed.to_rfc3339(), "1900-01-01T00:00:00+00:00", "{text:?}");
        }
        // Near-misses stay invalid (exact match on the raw text, both
        // sides probed — padding fails even though this parser trims
        // some padded datetimes).
        for text in [
            "iso8601",
            "xiso-8601",
            "iso-8601x",
            " iso-8601",
            "iso-8601 ",
            "iso-8601\n",
            "\tiso-8601",
        ] {
            assert!(parse_drf_datetime(text, &utc).is_err(), "{text:?}");
        }
    }

    #[test]
    fn uuid_int_path_covers_128_bits() {
        // `2^100` takes `int=` (then misses the lookup); past-`2^128`
        // magnitudes and negatives are invalid, echoing exactly.
        let big = num("1267650600228229401496703205376");
        assert_eq!(parse_uuid_pk(&big), Ok(Uuid::from_u128(1 << 100)));
        let past = num("340282366920938463463374607431768211456");
        assert!(parse_uuid_pk(&past).is_err());
        assert!(parse_uuid_pk(&num("-5")).is_err());
    }

    #[test]
    fn null_twins_win_over_explicit_ids() {
        // `{"parent": null, "parent_id": "<uuid>"}` clears (DRF's later
        // field overwrites `attrs`, even with null) — it must not fall
        // back to the twin's value.
        let twin = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let other = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
        let mut validated: BTreeMap<&'static str, Validated> = BTreeMap::new();
        validated.insert("parent", Validated::Null);
        validated.insert("parent_id", Validated::Uuid(twin));
        assert_eq!(v_parent(&validated), None);
        validated.insert("parent", Validated::Uuid(other));
        assert_eq!(v_parent(&validated), Some(other));
        validated.remove("parent");
        assert_eq!(v_parent(&validated), Some(twin));
    }

    #[test]
    fn create_response_follows_values_order() {
        // WIRE order, not the `base.py:433-461` call order: Django's
        // compiler emits concrete `.values()` fields first (call order),
        // then annotations in definition order (`apply_annotations`
        // cycle/link/attachment/sub-count, then the grouper's
        // assignee/label/module ids). Verified against live creates.
        assert_eq!(
            CREATE_RESPONSE_FIELDS.as_slice(),
            [
                "id",
                "name",
                "state_id",
                "sort_order",
                "completed_at",
                "estimate_point",
                "priority",
                "start_date",
                "target_date",
                "sequence_id",
                "project_id",
                "parent_id",
                "created_at",
                "updated_at",
                "created_by",
                "updated_by",
                "is_draft",
                "archived_at",
                "deleted_at",
                "cycle_id",
                "link_count",
                "attachment_count",
                "sub_issues_count",
                "assignee_ids",
                "label_ids",
                "module_ids",
            ]
            .as_slice()
        );
    }
}
