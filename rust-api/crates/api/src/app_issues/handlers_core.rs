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
//! `capture_prior_state` + `fire_state_transition`, plus the `github_signals`
//! completion-comment leg (`trigger_completion_comment`, which enqueues
//! `post_completion_comment` when a mirrored issue transitions into a
//! completed state — PIDASHCONV-1460, via the shared
//! `crate::issue_transition::enqueue_completion_comment` helper, before
//! the orchestration fire).
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
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use sea_orm::sea_query::extension::postgres::PgFunc;
use sea_orm::sea_query::{Alias, Asterisk, Expr, Func, Query, SelectStatement, SimpleExpr};
use sea_orm::{
    ColumnTrait, Condition, DbErr, EntityTrait, FromQueryResult, JoinType, PaginatorTrait,
    QueryFilter, QueryOrder, QueryResult, QuerySelect, RelationTrait,
};
use serde_json::{Map, Value};
use uuid::Uuid;

use pidash_auth::permissions::allow::{
    decide_allow, AllowFacts, AllowLevel, AllowSpec, CreatorGate,
};
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_auth::scope::TenantScope;
use pidash_db::entity::{
    assistant_user_llm_config, cycle_issues, estimate_points, file_assets, issue_assignees,
    issue_labels, issue_links, issue_reactions, issue_subscribers, issue_types, issues, labels,
    module_issues, modules, pod, project_issue_types, project_members, projects, states, users,
    workspaces,
};
use pidash_db::tasks_ticker::models::issue_agent_ticker::{tickers_for_issue, IssueAgentTicker};
use pidash_services::app_issues::agent_runs::{self, RenderedAgentRun};
use pidash_services::app_issues::serializers_create::{
    self, estimate_exists, fetch_pod_ref, fetch_resolved_project, filter_assignee_members,
    filter_project_labels, issue_create_to_representation, issue_create_validate, parent_exists,
    state_exists, CreateValidateError, ExecutorPolicy, IssueCreateRow, LockedIssueAttrs,
    LockedIssueValues, PodRef, ResolvedProject, ValidateAttrs, ValidateContext, ValidateInstance,
    ValidateProbes, ValidatedAttrs,
};
use pidash_services::app_issues::serializers_detail::{
    issue_detail_to_representation, serialize_drf_datetime, AgentTickerInput, IssueDetailBaseRow,
    IssueDetailRow,
};
use pidash_services::app_issues::serializers_engage;
use pidash_services::auth_session::shapes::{base_host, HostSettings};
use pidash_services::dispatch::admission::{
    managed_runner_availability, managed_runner_probe, LlmProfile, ManagedRunnerProbe,
    HEARTBEAT_GRACE_SECS,
};
use pidash_services::dispatch::policy::{
    cloud_agent_is_configured, managed_runner_is_enabled, UserFlags,
};
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::issues as domain_issues;
use pidash_services::domain::issues::IssueFieldSet as IssueSaveRow;
use pidash_services::domain::transition::IssueTransitionHook;
use pidash_services::domain::{self, WriteActor, WriteCtx, WriteScope};
use pidash_services::orchestration::clock::ProjectClockPolicy;
use pidash_services::read;
use pidash_types::dispatch::{AgentExecutorKind, ManagedRunnerReason};
use pidash_types::orchestration::StateRef;
use pidash_types::WorkspaceId;

use super::access;
use super::{
    actor_user_id, json_response, resolve_project_id, Denial, HandlerResult, NOT_FOUND_BODY,
};
use crate::domain_hooks::AppTaskEnqueueHook;
use crate::issue_transition::AppTransitionHook;
use crate::middleware::body_limit::read_request_bytes;
use crate::middleware::SessionHandle;
use crate::serializer::render_datetime_in;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json::{
    parse_request_data, to_serde_publish, JsonFail, JSON_PARSE_PREFIX,
};
use pidash_compat::datetime::{
    parse_digits, parse_iso_date_part, parse_iso_datetime, split_tz_suffix,
};
use pidash_compat::response::{json_created, no_content as empty_response};

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

const RECENT_VISITED_TASK: &str = "pi_dash.bgtasks.recent_visited_task.recent_visited_task";
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
    let user: Option<(String, bool, bool)> = users::Entity::find()
        .select_only()
        .columns([
            users::Column::UserTimezone,
            users::Column::IsActive,
            users::Column::IsBot,
        ])
        .filter(users::Column::Id.eq(*user_id))
        .into_tuple()
        .one(&orm(pool))
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
) -> Result<AllowFacts, DbErr> {
    let db = orm(pool);
    let member = access::workspace_role(&db, slug, *user_id).await?.is_some();
    let admin = access::is_workspace_admin(&db, slug, *user_id).await?;
    let any_project = access::project_role(&db, slug, *project_id, *user_id)
        .await?
        .is_some();
    let roles: Vec<i16> = gate.roles.iter().map(|role| *role as i16).collect();
    let allowed_project: Option<Uuid> = project_members::Entity::find()
        .select_only()
        .column(project_members::Column::Id)
        .join(
            JoinType::InnerJoin,
            project_members::Relation::Workspaces.def(),
        )
        .filter(project_members::Column::MemberId.eq(*user_id))
        .filter(project_members::Column::ProjectId.eq(*project_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(project_members::Column::Role.is_in(roles))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&db)
        .await?;
    let creator = match (gate.creator_bypass, pk) {
        (true, Some(pk)) => {
            let row: Option<Uuid> = issues::Entity::find()
                .select_only()
                .column(issues::Column::Id)
                .filter(issues::Column::Id.eq(pk))
                .filter(issues::Column::CreatedById.eq(*user_id))
                .filter(issues::Column::DeletedAt.is_null())
                .into_tuple()
                .one(&db)
                .await?;
            row.is_some()
        }
        _ => false,
    };
    Ok(AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: member,
        has_allowed_workspace_role: false,
        is_creator: creator,
        has_allowed_project_role: allowed_project.is_some(),
        is_project_member: any_project,
        is_workspace_admin: admin,
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
    let mut query = projects::Entity::find()
        .select_only()
        .columns([
            projects::Column::WorkspaceId,
            projects::Column::DefaultAssigneeId,
            projects::Column::GuestViewAllFeatures,
        ])
        .filter(projects::Column::Id.eq(*project_id))
        .filter(projects::Column::DeletedAt.is_null());
    if check == ProjectCheck::InSlug {
        query = query
            .join(JoinType::InnerJoin, projects::Relation::Workspaces.def())
            .filter(workspaces::Column::Slug.eq(slug));
    }
    let project: Option<(Uuid, Option<Uuid>, bool)> = query
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    let (workspace_id, default_assignee_id, guest_view) = project.ok_or(Denial::NotFound)?;
    Ok((*project_id, workspace_id, default_assignee_id, guest_view))
}

/// The SeaORM connection over a pool (a wrapper; no I/O).
fn orm(pool: &sqlx::PgPool) -> sea_orm::DatabaseConnection {
    pidash_db::pool::sea_orm_connection(pool)
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

fn not_found_detail(body: &str, status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_owned()))
        .expect("detail response")
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
pub(crate) fn parse_drf_date(text: &str) -> Option<NaiveDate> {
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
async fn pk_exists(pool: &sqlx::PgPool, queryset: PkQueryset, id: &Uuid) -> Result<bool, DbErr> {
    let db = orm(pool);
    let id = *id;
    let hit: Option<Uuid> = match queryset {
        PkQueryset::StatesAll | PkQueryset::States => {
            return Ok(state_row(pool, queryset, &id).await?.is_some());
        }
        PkQueryset::IssuesAll | PkQueryset::Issues => {
            let mut query = issues::Entity::find()
                .select_only()
                .column(issues::Column::Id)
                .filter(issues::Column::Id.eq(id));
            if queryset == PkQueryset::Issues {
                query = query.filter(issues::Column::DeletedAt.is_null());
            }
            query.into_tuple().one(&db).await?
        }
        PkQueryset::PodsAll => {
            pod::Entity::find()
                .select_only()
                .column(pod::Column::Id)
                .filter(pod::Column::Id.eq(id))
                .into_tuple()
                .one(&db)
                .await?
        }
        PkQueryset::Labels => {
            labels::Entity::find()
                .select_only()
                .column(labels::Column::Id)
                .filter(labels::Column::Id.eq(id))
                .filter(labels::Column::DeletedAt.is_null())
                .into_tuple()
                .one(&db)
                .await?
        }
        PkQueryset::Users => {
            users::Entity::find()
                .select_only()
                .column(users::Column::Id)
                .filter(users::Column::Id.eq(id))
                .into_tuple()
                .one(&db)
                .await?
        }
        PkQueryset::Estimates => {
            estimate_points::Entity::find()
                .select_only()
                .column(estimate_points::Column::Id)
                .filter(estimate_points::Column::Id.eq(id))
                .filter(estimate_points::Column::DeletedAt.is_null())
                .into_tuple()
                .one(&db)
                .await?
        }
        PkQueryset::Types => {
            issue_types::Entity::find()
                .select_only()
                .column(issue_types::Column::Id)
                .filter(issue_types::Column::Id.eq(id))
                .filter(issue_types::Column::DeletedAt.is_null())
                .into_tuple()
                .one(&db)
                .await?
        }
    };
    Ok(hit.is_some())
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

/// 400 with a pre-rendered JSON body (mirrors the sibling handlers).
fn json_bad_request(body: &str) -> Response {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_owned()))
        .expect("bad-request response")
}

/// `Pod.all_objects.get(pk)`: tombstones included so `validate()` can
/// return the friendly deleted message.
async fn pod_ref(pool: &sqlx::PgPool, id: &Uuid) -> Result<Option<PodRef>, DbErr> {
    fetch_pod_ref(&orm(pool), *id).await
}

async fn git_issue_synced(pool: &sqlx::PgPool, issue_id: &Uuid) -> Result<bool, DbErr> {
    serializers_engage::git_issue_synced(&orm(pool), *issue_id).await
}

async fn github_issue_synced(pool: &sqlx::PgPool, issue_id: &Uuid) -> Result<bool, DbErr> {
    serializers_engage::github_issue_synced(&orm(pool), *issue_id).await
}

async fn has_active_run(pool: &sqlx::PgPool, issue_id: &Uuid) -> Result<bool, DbErr> {
    serializers_create::has_active_run(&orm(pool), *issue_id).await
}

async fn filter_assignee_ids(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, DbErr> {
    filter_assignee_members(&orm(pool), *project_id, ids).await
}

async fn filter_label_ids(
    pool: &sqlx::PgPool,
    project_id: Option<&Uuid>,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, DbErr> {
    filter_project_labels(&orm(pool), project_id.copied(), ids).await
}

/// The step 9-11 project-scoped `EXISTS`: Django's `IS NULL` rendering
/// when the context project is `None` (PUT).
#[derive(Clone, Copy)]
enum ScopedProbe {
    State,
    Parent,
    Estimate,
}

async fn project_scoped_exists(
    pool: &sqlx::PgPool,
    probe: ScopedProbe,
    project_id: Option<&Uuid>,
    id: &Uuid,
) -> Result<bool, DbErr> {
    let db = orm(pool);
    let project_id = project_id.copied();
    match probe {
        ScopedProbe::State => state_exists(&db, project_id, *id).await,
        ScopedProbe::Parent => parent_exists(&db, project_id, *id).await,
        ScopedProbe::Estimate => estimate_exists(&db, project_id, *id).await,
    }
}

/// `UserLLMConfig.objects.filter(user=user).first()` for the
/// `managed_llm_profile` seam (`model_provider.py:67-68`, `:85-91`): no
/// model ordering, so an unordered first row; the verdict is
/// `bool(api_key_encrypted)`.
pub(crate) async fn assistant_has_api_key(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
) -> Result<bool, DbErr> {
    let row: Option<Option<Vec<u8>>> = assistant_user_llm_config::Entity::find()
        .select_only()
        .column(assistant_user_llm_config::Column::ApiKeyEncrypted)
        .filter(assistant_user_llm_config::Column::UserId.eq(*user_id))
        .into_tuple()
        .one(&orm(pool))
        .await?;
    Ok(row.flatten().is_some_and(|key| !key.is_empty()))
}

async fn fetch_project_ws(
    pool: &sqlx::PgPool,
    id: &Uuid,
) -> Result<Option<ResolvedProject>, DbErr> {
    fetch_resolved_project(&orm(pool), *id).await
}

pub(crate) async fn enrolled_runner(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
) -> Result<bool, DbErr> {
    let hit = managed_runner_probe(
        &orm(pool),
        ManagedRunnerProbe::Enrolled,
        *user_id,
        *project_id,
        *workspace_id,
    )
    .await?;
    Ok(hit.is_some())
}

pub(crate) async fn online_runner(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
) -> Result<bool, DbErr> {
    let bound = Utc::now() - chrono::Duration::seconds(HEARTBEAT_GRACE_SECS);
    let hit = managed_runner_probe(
        &orm(pool),
        ManagedRunnerProbe::Online(bound),
        *user_id,
        *project_id,
        *workspace_id,
    )
    .await?;
    Ok(hit.is_some())
}

/// The paused tiebreaker (`policy.py:88-91`, `09738d222`).
pub(crate) async fn paused_runner(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    project_id: &Uuid,
    workspace_id: &Uuid,
) -> Result<bool, DbErr> {
    let hit = managed_runner_probe(
        &orm(pool),
        ManagedRunnerProbe::Paused,
        *user_id,
        *project_id,
        *workspace_id,
    )
    .await?;
    Ok(hit.is_some())
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
    let mut paused_exists = false;
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
                                            // `09738d222`: the paused
                                            // tiebreaker runs only when
                                            // nothing is online.
                                            if !online_exists {
                                                paused_exists = paused_runner(
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
                                }
                                let verdict = managed_runner_availability(
                                    &state.settings().managed_runner,
                                    Some(&viewer),
                                    || llm_profile.clone(),
                                    enrolled_exists,
                                    online_exists,
                                    paused_exists,
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
    // query TWICE when both arms are reached. #2903 compares the
    // normalized write against the stored value, so the probe issues on
    // any executor write against an instance (even on caller normalization).
    let midflight_reached =
        !pod_returns && agent_executor.is_some() && instance.is_some() && !executor_returns;
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
                ScopedProbe::State,
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
                ScopedProbe::Parent,
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
            project_scoped_exists(pool, ScopedProbe::Estimate, project_id.as_ref(), &id)
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
        paused_exists: &|| paused_exists,
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

// ---------------------------------------------------------------------------
// `Issue.save()` port: defaults, recomputes, sequence/sort, the write
// ---------------------------------------------------------------------------

/// The state field `get()`: same guards as [`pk_exists`], but the row's
/// group rides along for the save's `completed_at` recompute.
async fn state_row(
    pool: &sqlx::PgPool,
    queryset: PkQueryset,
    id: &Uuid,
) -> Result<Option<(Uuid, String)>, DbErr> {
    let mut query = states::Entity::find()
        .select_only()
        .columns([states::Column::Id, states::Column::Group])
        .filter(states::Column::Id.eq(*id));
    match queryset {
        PkQueryset::StatesAll => {}
        PkQueryset::States => {
            query = query
                .filter(states::Column::DeletedAt.is_null())
                .filter(states::Column::Group.ne("triage"));
        }
        _ => return Ok(None),
    }
    query.into_tuple().one(&orm(pool)).await
}

/// `self.project` on a save (`issue.py:293-296` + the lock key): the
/// issue's FK cache is cold on both paths, so Django fetches the row. A
/// miss raises `DoesNotExist` → the required-object 404.
async fn touch_save_project(pool: &sqlx::PgPool, project_id: &Uuid) -> Result<(), Denial> {
    let row: Option<Uuid> = projects::Entity::find()
        .select_only()
        .column(projects::Column::Id)
        .filter(projects::Column::Id.eq(*project_id))
        .filter(projects::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|_| ()).ok_or(Denial::NotFound)
}

/// The domain-write context for an app issue request: the request user as
/// actor (with their stored time zone name) plus the gated tenant scope.
/// The origin stays `None` — origins resolve per effect at their original
/// points (see `domain::issues`).
fn write_ctx(tenant: &CoreTenant, user_id: &Uuid) -> WriteCtx {
    WriteCtx::new(WriteActor {
        user_id: *user_id,
        timezone: Some(tenant.timezone.name().to_owned()),
    })
    .with_scope(WriteScope {
        workspace_id: Some(tenant.workspace_id),
        project_id: Some(tenant.project_id),
    })
}

/// Map a domain-write failure onto the save arms: a dangling-row miss
/// 404s, an integrity violation is the invalid-payload 400, anything else
/// 500s.
fn domain_denial(error: domain::DomainError) -> Denial {
    if error.is_not_found() {
        Denial::NotFound
    } else if error.is_integrity_violation() {
        Denial::BadError("The payload is not valid".to_owned())
    } else {
        Denial::ServerError
    }
}

// The save row is the domain's `IssueFieldSet` (imported at the top):
// every `issues` column as owned values. Assembly, overlay and fetchers
// keep building it here (validation-adjacent); the INSERT / full-column
// UPDATE moved to `domain::issues` (ORM-03).

// ---------------------------------------------------------------------------
// Task enqueues (best-effort: failures never change the response)
// ---------------------------------------------------------------------------

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
    let enqueue = AppTaskEnqueueHook::new(pool);
    enqueue
        .enqueue_task(RECENT_VISITED_TASK, vec![], kwargs)
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

/// Which `assignee_ids` leg an annotated lookup renders: retrieve / PATCH
/// join the active-member rows (`base.py:534-545`; the `users` and
/// `project_members` joins carry no soft-delete guard, and the membership
/// need not be in this project, exactly as Django compiles it); the create
/// post-query goes through `issue_queryset_grouper` (live links only).
#[derive(Clone, Copy, PartialEq, Eq)]
enum AssigneeLeg {
    ActiveMember,
    Plain,
}

/// The lookup scope: the live-only `SoftDeletionManager`, or
/// `issue_objects` (live rows, non-triage state with NULL-state rows KEPT —
/// the null-safe `exclude(state__group="triage")` — unarchived issue and
/// project, non-draft).
#[derive(Clone, Copy, PartialEq, Eq)]
enum IssueScope {
    Live,
    IssueObjects,
}

/// The `issue_objects` conjuncts past `deleted_at` (needs the `states`
/// LEFT JOIN and the `projects` INNER JOIN).
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

/// The issue row behind every lookup: `pk` in this project and workspace
/// slug, under `scope`. `.first()` / `.get()` on a pk: the
/// `-created_at` order is kept, and at most one row can match.
fn scoped_issue_select(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    scope: IssueScope,
) -> sea_orm::Select<issues::Entity> {
    let mut query = issues::Entity::find()
        .join(JoinType::InnerJoin, issues::Relation::Workspaces.def())
        .filter(issues::Column::DeletedAt.is_null())
        .filter(issues::Column::Id.eq(issue_id))
        .filter(issues::Column::ProjectId.eq(project_id))
        .filter(workspaces::Column::Slug.eq(slug));
    if scope == IssueScope::IssueObjects {
        query = query
            .join(JoinType::LeftJoin, issues::Relation::States.def())
            .join(JoinType::InnerJoin, issues::Relation::Projects.def())
            .filter(issue_objects_condition());
    }
    query.order_by_desc(issues::Column::CreatedAt)
}

async fn fetch_scoped_issue(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    scope: IssueScope,
) -> Result<Option<IssueSaveRow>, Denial> {
    let row = scoped_issue_select(slug, *project_id, *issue_id, scope)
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.as_ref().map(IssueSaveRow::from))
}

/// The scalar and id-list annotations of one issue (`base.py:496-558`,
/// the create post-query's `apply_annotations` + grouper ids).
struct IssueAnnotations {
    cycle_id: Option<Uuid>,
    link_count: Option<i64>,
    attachment_count: Option<i64>,
    sub_issues_count: Option<i64>,
    label_ids: Vec<Uuid>,
    assignee_ids: Vec<Uuid>,
    module_ids: Vec<Uuid>,
}

/// Django's grouped `COUNT(id)` subquery yields no row (SQL `NULL`) when
/// nothing matches; the subquery here counts, and zero reads as `None`.
fn null_if_zero(count: i64) -> Option<i64> {
    (count > 0).then_some(count)
}

/// `ArrayAgg(..., distinct=True)` with the empty-array default: an empty
/// aggregate is SQL `NULL` → `[]`. Postgres sorts a `DISTINCT` aggregate's
/// input; the re-sort pins that order (`Uuid` orders by bytes, as
/// Postgres's `uuid` does) without relying on the executor.
fn distinct_ids(ids: Option<Vec<Uuid>>) -> Vec<Uuid> {
    ids.unwrap_or_default()
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The outer row's `issues.id`, the correlation key of every annotation.
fn outer_issue_id() -> SimpleExpr {
    Expr::col((issues::Entity, issues::Column::Id)).into()
}

fn scalar(query: SelectStatement) -> SimpleExpr {
    SimpleExpr::SubQuery(None, Box::new(query.into_sub_query_statement()))
}

/// `(SELECT COUNT(*) FROM <query>)` as a scalar subquery.
fn count_of(mut query: SelectStatement) -> SimpleExpr {
    query.expr(Func::count(Expr::col(Asterisk)));
    scalar(query)
}

/// `(SELECT ARRAY_AGG(DISTINCT <column>) FROM <query>)`.
fn distinct_array_of(mut query: SelectStatement, column: SimpleExpr) -> SimpleExpr {
    query.expr(PgFunc::array_agg_distinct(column));
    scalar(query)
}

/// Appends the seven annotations to an issue select as correlated
/// subqueries on the outer `issues.id`, so the lookup stays one statement.
fn annotate_issue_select(
    query: sea_orm::Select<issues::Entity>,
    assignees: AssigneeLeg,
) -> sea_orm::Select<issues::Entity> {
    // `cycle_id`: at most one live link per issue (the cycle write paths
    // move links), so the dropped model ordering cannot change the row.
    let cycle_id = scalar(
        Query::select()
            .column((cycle_issues::Entity, cycle_issues::Column::CycleId))
            .from(cycle_issues::Entity)
            .and_where(
                Expr::col((cycle_issues::Entity, cycle_issues::Column::IssueId))
                    .eq(outer_issue_id()),
            )
            .and_where(Expr::col((cycle_issues::Entity, cycle_issues::Column::DeletedAt)).is_null())
            .limit(1)
            .to_owned(),
    );
    let link_count = count_of(
        Query::select()
            .from(issue_links::Entity)
            .and_where(
                Expr::col((issue_links::Entity, issue_links::Column::IssueId)).eq(outer_issue_id()),
            )
            .and_where(Expr::col((issue_links::Entity, issue_links::Column::DeletedAt)).is_null())
            .to_owned(),
    );
    let attachment_count = count_of(
        Query::select()
            .from(file_assets::Entity)
            .and_where(
                Expr::col((file_assets::Entity, file_assets::Column::IssueId)).eq(outer_issue_id()),
            )
            .and_where(
                Expr::col((file_assets::Entity, file_assets::Column::EntityType))
                    .eq("ISSUE_ATTACHMENT"),
            )
            .and_where(Expr::col((file_assets::Entity, file_assets::Column::DeletedAt)).is_null())
            .to_owned(),
    );
    // `sub_issues_count` over the children's `issue_objects` scope; the
    // child tables are aliased off the outer `issues`/`states`/`projects`.
    let (child, child_state, child_project) = (Alias::new("c"), Alias::new("cs"), Alias::new("cp"));
    let sub_issues_count = count_of(
        Query::select()
            .from_as(issues::Entity, child.clone())
            .join_as(
                JoinType::LeftJoin,
                states::Entity,
                child_state.clone(),
                Expr::col((child_state.clone(), states::Column::Id))
                    .equals((child.clone(), issues::Column::StateId)),
            )
            .join_as(
                JoinType::InnerJoin,
                projects::Entity,
                child_project.clone(),
                Expr::col((child_project.clone(), projects::Column::Id))
                    .equals((child.clone(), issues::Column::ProjectId)),
            )
            .and_where(Expr::col((child.clone(), issues::Column::ParentId)).eq(outer_issue_id()))
            .and_where(Expr::col((child.clone(), issues::Column::DeletedAt)).is_null())
            .cond_where(
                Condition::all()
                    .add(
                        Condition::all()
                            .add(
                                Expr::col((child_state.clone(), states::Column::Group))
                                    .eq("triage"),
                            )
                            .add(
                                Expr::col((child_state.clone(), states::Column::Group))
                                    .is_not_null(),
                            )
                            .not(),
                    )
                    .add(Expr::col((child.clone(), issues::Column::ArchivedAt)).is_null())
                    .add(Expr::col((child_project, projects::Column::ArchivedAt)).is_null())
                    .add(Expr::col((child, issues::Column::IsDraft)).eq(false)),
            )
            .to_owned(),
    );
    let label_ids = distinct_array_of(
        Query::select()
            .from(issue_labels::Entity)
            .and_where(
                Expr::col((issue_labels::Entity, issue_labels::Column::IssueId))
                    .eq(outer_issue_id()),
            )
            .and_where(Expr::col((issue_labels::Entity, issue_labels::Column::DeletedAt)).is_null())
            .to_owned(),
        Expr::col((issue_labels::Entity, issue_labels::Column::LabelId)).into(),
    );
    let mut assignee_query = Query::select()
        .from(issue_assignees::Entity)
        .and_where(
            Expr::col((issue_assignees::Entity, issue_assignees::Column::IssueId))
                .eq(outer_issue_id()),
        )
        .and_where(
            Expr::col((issue_assignees::Entity, issue_assignees::Column::DeletedAt)).is_null(),
        )
        .to_owned();
    if assignees == AssigneeLeg::ActiveMember {
        assignee_query
            .inner_join(
                users::Entity,
                Expr::col((users::Entity, users::Column::Id))
                    .equals((issue_assignees::Entity, issue_assignees::Column::AssigneeId)),
            )
            .inner_join(
                project_members::Entity,
                Expr::col((project_members::Entity, project_members::Column::MemberId))
                    .equals((users::Entity, users::Column::Id)),
            )
            .and_where(
                Expr::col((project_members::Entity, project_members::Column::IsActive)).eq(true),
            );
    }
    let assignee_ids = distinct_array_of(
        assignee_query,
        Expr::col((issue_assignees::Entity, issue_assignees::Column::AssigneeId)).into(),
    );
    // `module_ids`: archived modules drop; no `modules.deleted_at` guard.
    let module_ids = distinct_array_of(
        Query::select()
            .from(module_issues::Entity)
            .inner_join(
                modules::Entity,
                Expr::col((modules::Entity, modules::Column::Id))
                    .equals((module_issues::Entity, module_issues::Column::ModuleId)),
            )
            .and_where(
                Expr::col((module_issues::Entity, module_issues::Column::IssueId))
                    .eq(outer_issue_id()),
            )
            .and_where(
                Expr::col((module_issues::Entity, module_issues::Column::DeletedAt)).is_null(),
            )
            .and_where(Expr::col((modules::Entity, modules::Column::ArchivedAt)).is_null())
            .to_owned(),
        Expr::col((module_issues::Entity, module_issues::Column::ModuleId)).into(),
    );
    query
        .expr_as(cycle_id, "cycle_id")
        .expr_as(link_count, "link_count")
        .expr_as(attachment_count, "attachment_count")
        .expr_as(sub_issues_count, "sub_issues_count")
        .expr_as(label_ids, "label_ids")
        .expr_as(assignee_ids, "assignee_ids")
        .expr_as(module_ids, "module_ids")
}

/// Retrieve's `is_subscribed` `Exists` (`base.py:571-580`): a live
/// subscription in this project and workspace slug, correlated on the
/// outer issue (the subscriber's workspace aliased off the outer join).
fn is_subscribed_expr(slug: &str, project_id: Uuid, user_id: Uuid) -> SimpleExpr {
    let sub_ws = Alias::new("sw");
    Expr::exists(
        Query::select()
            .expr(Expr::val(1))
            .from(issue_subscribers::Entity)
            .join_as(
                JoinType::InnerJoin,
                workspaces::Entity,
                sub_ws.clone(),
                Expr::col((sub_ws.clone(), workspaces::Column::Id)).equals((
                    issue_subscribers::Entity,
                    issue_subscribers::Column::WorkspaceId,
                )),
            )
            .and_where(
                Expr::col((
                    issue_subscribers::Entity,
                    issue_subscribers::Column::DeletedAt,
                ))
                .is_null(),
            )
            .and_where(
                Expr::col((
                    issue_subscribers::Entity,
                    issue_subscribers::Column::IssueId,
                ))
                .eq(outer_issue_id()),
            )
            .and_where(
                Expr::col((
                    issue_subscribers::Entity,
                    issue_subscribers::Column::ProjectId,
                ))
                .eq(project_id),
            )
            .and_where(
                Expr::col((
                    issue_subscribers::Entity,
                    issue_subscribers::Column::SubscriberId,
                ))
                .eq(user_id),
            )
            .and_where(Expr::col((sub_ws, workspaces::Column::Slug)).eq(slug))
            .limit(1)
            .to_owned(),
    )
}

/// One annotated lookup row, decoded by name: the issue model plus the
/// aliased annotation columns (`is_subscribed` / state pair only when
/// selected).
struct AnnotatedIssueRow {
    issue: issues::Model,
    annotations: IssueAnnotations,
    is_subscribed: Option<bool>,
    state_group: Option<String>,
    state_name: Option<String>,
}

impl FromQueryResult for AnnotatedIssueRow {
    fn from_query_result(res: &QueryResult, pre: &str) -> Result<Self, DbErr> {
        let count = |name: &str| -> Result<Option<i64>, DbErr> {
            Ok(res
                .try_get::<Option<i64>>(pre, name)?
                .and_then(null_if_zero))
        };
        let ids = |name: &str| -> Result<Vec<Uuid>, DbErr> {
            Ok(distinct_ids(res.try_get::<Option<Vec<Uuid>>>(pre, name)?))
        };
        Ok(Self {
            issue: issues::Model::from_query_result(res, pre)?,
            annotations: IssueAnnotations {
                cycle_id: res.try_get(pre, "cycle_id")?,
                link_count: count("link_count")?,
                attachment_count: count("attachment_count")?,
                sub_issues_count: count("sub_issues_count")?,
                label_ids: ids("label_ids")?,
                assignee_ids: ids("assignee_ids")?,
                module_ids: ids("module_ids")?,
            },
            is_subscribed: res.try_get(pre, "is_subscribed").ok(),
            state_group: res.try_get(pre, "state_group").ok().flatten(),
            state_name: res.try_get(pre, "state_name").ok().flatten(),
        })
    }
}

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

/// The annotated issue lookup as one statement: retrieve's queryset
/// (live-only scope + member-active assignees + `is_subscribed`) or the
/// PATCH shape (`issue_objects` scope, no `is_subscribed`). The state pair
/// comes off the `states` LEFT JOIN (no soft-delete guard, as Django
/// compiles it).
fn annotated_issue_select(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    user_id: Uuid,
    issue_objects: bool,
) -> sea_orm::Select<issues::Entity> {
    let scope = if issue_objects {
        IssueScope::IssueObjects
    } else {
        IssueScope::Live
    };
    let mut query = scoped_issue_select(slug, project_id, issue_id, scope);
    if !issue_objects {
        // The `issue_objects` scope already LEFT JOINs `states`.
        query = query.join(JoinType::LeftJoin, issues::Relation::States.def());
    }
    query = annotate_issue_select(query, AssigneeLeg::ActiveMember)
        .expr_as(
            Expr::col((states::Entity, states::Column::Group)),
            "state_group",
        )
        .expr_as(
            Expr::col((states::Entity, states::Column::Name)),
            "state_name",
        );
    if !issue_objects {
        query = query.expr_as(
            is_subscribed_expr(slug, project_id, user_id),
            "is_subscribed",
        );
    }
    query
}

async fn fetch_annotated_issue(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
    user_id: &Uuid,
    issue_objects: bool,
) -> Result<Option<CoreIssueRow>, Denial> {
    let row = annotated_issue_select(slug, *project_id, *issue_id, *user_id, issue_objects)
        .into_model::<AnnotatedIssueRow>()
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let annotations = row.annotations;
    Ok(Some(CoreIssueRow {
        save: IssueSaveRow::from(&row.issue),
        cycle_id: annotations.cycle_id,
        link_count: annotations.link_count,
        attachment_count: annotations.attachment_count,
        sub_issues_count: annotations.sub_issues_count,
        label_ids: annotations.label_ids,
        assignee_ids: annotations.assignee_ids,
        module_ids: annotations.module_ids,
        is_subscribed: row.is_subscribed.unwrap_or(false),
        state_group: row.state_group,
        state_name: row.state_name,
    }))
}

/// `Issue.objects.get(workspace__slug, project_id, pk)`: the destroy
/// lookup — live-only, no annotations.
async fn fetch_destroy_issue(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<Option<IssueSaveRow>, Denial> {
    fetch_scoped_issue(pool, slug, project_id, issue_id, IssueScope::Live).await
}

/// The PUT lookup: the plain `get_queryset` (`issue_objects`, no
/// annotations — DRF's `get_object` never applies them).
async fn fetch_put_issue(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<Option<IssueSaveRow>, Denial> {
    fetch_scoped_issue(pool, slug, project_id, issue_id, IssueScope::IssueObjects).await
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

/// The `validate()`-consumed snapshot (annotation-less lookups). A free
/// function: the row is the domain's `IssueFieldSet`, which cannot carry
/// an inherent impl from this crate.
fn write_instance_of_save(row: &IssueSaveRow) -> WriteInstance {
    WriteInstance {
        id: row.id,
        project_id: row.project_id,
        workspace_id: row.workspace_id,
        external_source: row.external_source.clone(),
        agent_executor: row.agent_executor.clone(),
        assigned_pod_id: row.assigned_pod_id,
        name: row.name.clone(),
        description_html: row.description_html.clone(),
        description_json: row.description_json.clone(),
        description_stripped: row.description_stripped.clone(),
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
    let db = orm(pool);
    let _: Vec<Uuid> = issue_reactions::Entity::find()
        .select_only()
        .column(issue_reactions::Column::Id)
        .filter(issue_reactions::Column::IssueId.eq(*issue_id))
        .filter(issue_reactions::Column::DeletedAt.is_null())
        .into_tuple()
        .all(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let _: Vec<Uuid> = issue_links::Entity::find()
        .select_only()
        .column(issue_links::Column::Id)
        .filter(issue_links::Column::IssueId.eq(*issue_id))
        .filter(issue_links::Column::DeletedAt.is_null())
        .into_tuple()
        .all(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `obj.agent_ticker` (`issue.py:1275-1281`): the reverse OneToOne read
/// over live rows — a miss renders `agent_ticker: null` (and, without
/// runs, a null `agent_status` too).
async fn fetch_ticker(
    pool: &sqlx::PgPool,
    issue_id: &Uuid,
) -> Result<Option<IssueAgentTicker>, Denial> {
    let rows = tickers_for_issue(&orm(pool), *issue_id)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(rows.into_iter().find(|ticker| ticker.deleted_at.is_none()))
}

/// The clock policy behind `effective_max_ticks` /
/// `effective_interval_seconds` (`ticker.issue.project`): the consumed
/// project columns, read only past a ticker row. (Django re-fetches the
/// issue + state intermediaries on the way; those rows are already in
/// hand with identical values, so the reads narrow to the project.) The
/// shared `orchestration::entries::clock_policy` carries no soft-delete
/// guard, and this path re-fetches through the live-only manager, so it
/// keeps its own guarded read.
async fn fetch_clock_policy(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
) -> Result<ProjectClockPolicy, Denial> {
    let row: Option<(bool, i32, i32, i32, i32)> = projects::Entity::find()
        .select_only()
        .columns([
            projects::Column::AgentTickingEnabled,
            projects::Column::AgentDefaultMaxTicks,
            projects::Column::AgentDefaultIntervalSeconds,
            projects::Column::AgentReviewDefaultIntervalSeconds,
            projects::Column::AgentTestDefaultIntervalSeconds,
        ])
        .filter(projects::Column::Id.eq(*project_id))
        .filter(projects::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    // A missing project row raises on Django (`ticker.issue.project`
    // re-fetches through the live-only manager) → 500, never a default.
    let Some((ticking, max_ticks, interval, review_interval, test_interval)) = row else {
        return Err(Denial::ServerError);
    };
    Ok(ProjectClockPolicy {
        agent_ticking_enabled: Some(ticking),
        agent_default_max_ticks: Some(max_ticks),
        agent_default_interval_seconds: Some(i64::from(interval)),
        agent_review_default_interval_seconds: Some(i64::from(review_interval)),
        agent_test_default_interval_seconds: Some(i64::from(test_interval)),
    })
}

/// The issue's latest and active runs plus the run count through the
/// SeaORM agent-run reader. The live-state rule (`include_live_state`, the
/// observed-run guard) is applied by `agent_status_to_representation`.
async fn fetch_agent_runs(
    pool: &sqlx::PgPool,
    issue_id: &Uuid,
) -> Result<(Option<RenderedAgentRun>, Option<RenderedAgentRun>, i64), Denial> {
    let runs = agent_runs::issue_agent_runs(pool, *issue_id)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok((runs.latest, runs.active, runs.count))
}

/// The cached blocker summary via the public read service: both
/// direction lists plus the open-blockers `Exists`, in that order.
async fn fetch_blocker_summary(
    pool: &sqlx::PgPool,
    issue_id: &Uuid,
) -> Result<pidash_services::orchestration::blockers::RelationsSummary, Denial> {
    read::fetch_blocker_summary(pool, issue_id)
        .await
        .map_err(|_| Denial::ServerError)
}

/// `get_is_epic` (`issue.py:1109-1112`): false when the row is untyped,
/// without a query; otherwise the type row's flag. A missing row is
/// Django's `DoesNotExist` → the generic 500 (unreachable — the FK
/// holds). No soft-delete guard: `IssueType` rides the plain manager.
async fn fetch_is_epic(pool: &sqlx::PgPool, type_id: Option<Uuid>) -> Result<bool, Denial> {
    let Some(id) = type_id else {
        return Ok(false);
    };
    access::issue_type_is_epic(&orm(pool), id)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ServerError)
}

/// Render one `IssueDetailSerializer(...).data` body as an insertion-ordered
/// map: the 37-key retrieve form, or the 36-key `current_instance` form
/// without `is_subscribed` (the PATCH queryset never annotates it, so DRF
/// `SkipField`s the key). `is_intake` is absent on both (neither queryset
/// annotates it). The map form lets a caller append `?expand=` keys in the
/// same positions DRF's `to_representation` would.
async fn render_detail_map(
    pool: &sqlx::PgPool,
    row: &CoreIssueRow,
    timezone: &Tz,
    with_subscribed: bool,
    touch_prefetches: bool,
) -> Result<Map<String, Value>, Denial> {
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
    let type_id = save.type_id.map(|value| value.to_string());
    let is_epic = fetch_is_epic(pool, save.type_id).await?;
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
        type_id: type_id.as_deref(),
        is_epic,
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
    let latest_row = latest.as_ref().map(RenderedAgentRun::detail_row);
    let active_row = active.as_ref().map(RenderedAgentRun::detail_row);
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
    let value = serde_json::to_value(&view).map_err(|_| Denial::ServerError)?;
    let Value::Object(mut body) = value else {
        return Err(Denial::ServerError);
    };
    if !with_subscribed {
        // The PATCH queryset never annotates `is_subscribed`, so DRF
        // `SkipField`s the key before any expansion.
        body.remove("is_subscribed");
    }
    Ok(body)
}

/// Render one `IssueDetailSerializer(...).data` body as compact (or, when
/// `spaced`, `json.dumps`-spaced) JSON text.
async fn render_detail_body(
    pool: &sqlx::PgPool,
    row: &CoreIssueRow,
    timezone: &Tz,
    with_subscribed: bool,
    touch_prefetches: bool,
    spaced: bool,
) -> Result<String, Denial> {
    let body = render_detail_map(pool, row, timezone, with_subscribed, touch_prefetches).await?;
    let value = Value::Object(body);
    if spaced {
        domain_issues::to_spaced_json(&value).ok_or(Denial::ServerError)
    } else {
        serde_json::to_string(&value).map_err(|_| Denial::ServerError)
    }
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

/// The fresh-row assembly: the row and the raw m2m inputs. The state
/// decision (default assignment, `completed_at`) moved to the save hooks.
type NewIssueAssembly = (IssueSaveRow, Option<Vec<Uuid>>, Option<Vec<Uuid>>);

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
    let (_, state_id, _) = v_state(validated);
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
        // Audit stamps come from the domain write context, not the
        // assembly: `created_by` is the actor, `updated_by` stays `None`.
        created_by_id: None,
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
        // The normalized write (#2903): deprecated choices land local.
        agent_executor: filtered.agent_executor.clone().flatten(),
    };
    Ok((
        row,
        v_uuid_list(validated, "assignee_ids"),
        v_uuid_list(validated, "label_ids"),
    ))
}

/// `super().update(instance, validated_data)`: every present key applies
/// (`Null` clears); absent keys keep the row. `description_stripped`
/// input is skipped — `save()` recomputes it unconditionally. The state
/// decision (default assignment, `completed_at`) moved to the save hooks.
fn apply_validated_to_existing(
    row: &mut IssueSaveRow,
    validated: &BTreeMap<&'static str, Validated>,
    filtered: &ValidatedAttrs,
) -> Result<(), Denial> {
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
        // `attrs["agent_executor"]` holds the normalized write (#2903):
        // deprecated values land as local_runner.
        row.agent_executor = filtered.agent_executor.clone().flatten();
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
    // `state` overlays here; the default assignment (when the final id
    // is `None`) and the `completed_at` recompute run in the save hooks.
    let (touched, state_id, _) = v_state(validated);
    if touched {
        row.state_id = state_id;
    }
    Ok(())
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
) -> Result<Option<(IssueSaveRow, IssueAnnotations)>, Denial> {
    let query = scoped_issue_select(slug, *project_id, *issue_id, IssueScope::IssueObjects);
    let row = annotate_issue_select(query, AssigneeLeg::Plain)
        .into_model::<AnnotatedIssueRow>()
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| (IssueSaveRow::from(&row.issue), row.annotations)))
}

/// Render one `.values()` row in [`CREATE_RESPONSE_FIELDS`] wire order:
/// UUIDs as strings, `created_at`/`updated_at` in the actor's timezone
/// (the explicit `user_timezone_converter`), every other datetime UTC,
/// dates bare — exactly DRF's `JSONEncoder` over the dict.
fn render_create_response_body(
    row: &(IssueSaveRow, IssueAnnotations),
    timezone: &Tz,
) -> Result<String, Denial> {
    let (issue, annotations) = row;
    let uuid = |id: Option<Uuid>| id.map(|id| id.to_string());
    let ids = |ids: &[Uuid]| ids.iter().map(ToString::to_string).collect::<Vec<_>>();
    let date = |date: Option<NaiveDate>| date.map(|date| date.to_string());
    let get = |name: &str| -> Result<Value, Denial> {
        let value = match name {
            "id" => Value::from(issue.id.to_string()),
            "name" => Value::from(issue.name.clone()),
            "state_id" => Value::from(uuid(issue.state_id)),
            "sort_order" => {
                serde_json::to_value(issue.sort_order).map_err(|_| Denial::ServerError)?
            }
            "completed_at" => Value::from(issue.completed_at.map(serialize_drf_datetime)),
            "estimate_point" => Value::from(uuid(issue.estimate_point_id)),
            "priority" => Value::from(issue.priority.clone()),
            "start_date" => Value::from(date(issue.start_date)),
            "target_date" => Value::from(date(issue.target_date)),
            "sequence_id" => Value::from(issue.sequence_id),
            "project_id" => Value::from(issue.project_id.to_string()),
            "parent_id" => Value::from(uuid(issue.parent_id)),
            "created_at" => Value::from(py_iso_in(&issue.created_at, timezone)),
            "updated_at" => Value::from(py_iso_in(&issue.updated_at, timezone)),
            "created_by" => Value::from(uuid(issue.created_by_id)),
            "updated_by" => Value::from(uuid(issue.updated_by_id)),
            "is_draft" => Value::from(issue.is_draft),
            "archived_at" => Value::from(date(issue.archived_at)),
            "deleted_at" => Value::from(issue.deleted_at.map(serialize_drf_datetime)),
            "cycle_id" => Value::from(uuid(annotations.cycle_id)),
            "link_count" => Value::from(annotations.link_count),
            "attachment_count" => Value::from(annotations.attachment_count),
            "sub_issues_count" => Value::from(annotations.sub_issues_count),
            "assignee_ids" => Value::from(ids(&annotations.assignee_ids)),
            "label_ids" => Value::from(ids(&annotations.label_ids)),
            "module_ids" => Value::from(ids(&annotations.module_ids)),
            _ => return Err(Denial::ServerError),
        };
        Ok(value)
    };
    let mut out = String::from("{");
    for (index, field) in CREATE_RESPONSE_FIELDS.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let key = serde_json::to_string(field).map_err(|_| Denial::ServerError)?;
        out.push_str(&key);
        out.push(':');
        out.push_str(&serde_json::to_string(&get(field)?).map_err(|_| Denial::ServerError)?);
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
    let db = orm(pool);
    let assignees: Vec<Uuid> = users::Entity::find()
        .select_only()
        .column(users::Column::Id)
        .join(
            JoinType::InnerJoin,
            issue_assignees::Relation::Users3.def().rev(),
        )
        .filter(issue_assignees::Column::IssueId.eq(*issue_id))
        .order_by_desc(users::Column::CreatedAt)
        .into_tuple()
        .all(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let labels: Vec<Uuid> = labels::Entity::find()
        .select_only()
        .column(labels::Column::Id)
        .join(
            JoinType::InnerJoin,
            issue_labels::Relation::Labels.def().rev(),
        )
        .filter(issue_labels::Column::IssueId.eq(*issue_id))
        .filter(labels::Column::DeletedAt.is_null())
        .order_by_desc(labels::Column::CreatedAt)
        .into_tuple()
        .all(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok((assignees, labels))
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

/// Split a proxied-or-read request after the UUID guards passed. The read
/// is unbounded past the serve-wide `BodyLimitLayer`; a mid-stream
/// transport failure fails closed with the truncated-body 502.
async fn split_core_parts(
    req: axum::extract::Request,
) -> Result<(axum::http::request::Parts, Vec<u8>), Denial> {
    let (parts, body) = req.into_parts();
    let bytes = read_request_bytes(body)
        .await
        .map_err(|_| Denial::TruncatedBody)?;
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
    create_issue_inner(state, pool, tenant, user_id, slug, req, None).await
}

/// The `POST` pipeline after the gate: body split, validation, the atomic
/// sequence/sort INSERT, m2m + default-assignee legs, transition fire,
/// `issue_activity`, the post-query render, `model_activity` + the
/// creating version task. `forced_type` carries the epic create's resolved
/// type (`epics.py:82-85`); `None` is the plain issues create.
async fn create_issue_inner(
    state: AppState,
    pool: sqlx::PgPool,
    tenant: CoreTenant,
    user_id: Uuid,
    slug: String,
    req: axum::extract::Request,
    forced_type: Option<Uuid>,
) -> HandlerResult {
    let (parts, bytes) = split_core_parts(req).await?;
    let mut data = match negotiate_input(&parts.headers, &bytes) {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    // Epic creates copy the payload with `dict(request.data)` before
    // forcing the type (`epics.py:82`): the copy raises on non-dict
    // bodies, so epic non-dict input 500s where the plain create answers
    // `non_dict_response`.
    if forced_type.is_some() && data.value.as_object().is_none() {
        return Err(Denial::ServerError);
    }
    // A non-dict body fails the whole serializer (`non_field_errors`) —
    // the field loop never runs, so no `required` error joins it.
    if data.value.as_object().is_none() {
        return Ok(non_dict_response(&data.value));
    }
    // Epic creates force the resolved type onto the payload before
    // validation; it then rides validation, the activity dumps, and the
    // save like any client-supplied `type`.
    if let Some(type_id) = forced_type {
        if let Some(map) = data.value.as_object_mut() {
            map.insert("type".to_owned(), Value::String(type_id.to_string()));
        }
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
    let (row, raw_assignees, raw_labels) = assemble_new_issue(&validated, &filtered, &tenant)?;
    if row.state_id.is_none() {
        // The `self.project` touch (`issue.py:293-296` + the lock key):
        // Django fetches the project row on the NULL-state leg — a miss
        // is the required-object 404. A `SELECT`'s transaction membership
        // is unobservable, so it runs ahead of the domain write.
        touch_save_project(&pool, &tenant.project_id).await?;
    }
    // The m2m plan resolves pre-save (pure reads of validated input).
    let assignees = filtered
        .assignee_ids
        .clone()
        .or(raw_assignees)
        .unwrap_or_default();
    let labels = filtered
        .label_ids
        .clone()
        .or(raw_labels)
        .unwrap_or_default();
    // `base_host` raises past the save (the issue + no jobs persist on a
    // misconfigured host): `request_origin` is pure over settings, so it
    // resolves here but only takes effect past the fire + m2m legs — the
    // chain stops ahead of the activity when it is `None`.
    let origin = request_origin(&state).ok();
    let requested = domain_issues::to_spaced_json(&data.value).ok_or(Denial::ServerError)?;
    // The atomic sequence/sort INSERT (save hooks) plus the post-commit
    // chain — transition fire, m2m + default-assignee legs, created
    // activity — in handler order. Django's `post_save` fires inside
    // `Issue.objects.create`, before the assignee/label `bulk_create`s.
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = write_ctx(&tenant, &user_id);
    let transition: Arc<dyn IssueTransitionHook> = Arc::new(AppTransitionHook::new(&state, &pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pool));
    let outcome = domain::run(&ctx, orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let pool = pool.clone();
        let ctx = ctx.clone();
        let origin = origin.clone();
        let requested = requested.clone();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            domain_issues::create(
                &ctx,
                txn,
                &pool,
                domain_issues::CreateIssue {
                    fields: row,
                    assignees,
                    default_assignee: tenant.default_assignee_id,
                    labels,
                    fire: domain_issues::FireSpec {
                        dispatch_immediate: true,
                        created_by: Some(user_id),
                        moved_by_run: None,
                        now: utc_now_micros(),
                    },
                    origin: origin.clone(),
                    activity_requested: Value::String(requested.clone()),
                    activity_notification: Some(true),
                    activity_require_origin: true,
                    skip_activity: false,
                    skip_fire: false,
                    model_activity: None,
                    created_override: None,
                },
            )
            .await
        })
    })
    .await
    .map_err(domain_denial)?;
    if outcome.chain.stopped().is_some() {
        return Err(Denial::ServerError);
    }
    let issue_id = outcome.issue.id;
    let Some(post) = fetch_create_response_row(&pool, &slug, &tenant.project_id, &issue_id).await?
    else {
        // `user_timezone_converter(None)` → `TypeError` → the generic 500,
        // with the row + `issue_activity` already persisted.
        return Err(Denial::ServerError);
    };
    // A `None` origin stopped the chain above, so this is `Some` — the
    // `ok_or` is unreachable, kept for the type.
    let origin = origin.ok_or(Denial::ServerError)?;
    domain_issues::emit_model_activity(
        &enqueue,
        domain_issues::ModelActivitySpec {
            model: issue_id,
            requested: data.value.clone(),
            current: Value::Null,
            actor: user_id,
            slug: slug.clone(),
            origin: Some(origin),
        },
    )
    .await;
    domain_issues::emit_version_task(&enqueue, &requested, &issue_id, &user_id, Some(true)).await;
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
    let tenant = core_gate(
        &pool,
        &slug,
        &project_raw,
        &user_id,
        &GATE_PATCH,
        Some(pk),
        ProjectCheck::None,
    )
    .await?;
    partial_update_inner(state, pool, tenant, user_id, slug, pk, req, false).await
}

/// The `PATCH` pipeline after the gate: body split, `skip_activity` /
/// `is_description_update` pops, the annotated lookup, the pre-update
/// `current_instance` dump, validation, capture + save + fire, the three
/// task enqueues, 204. `epic` selects the epics-collection behavior
/// (`epics.py:186-193`): a `type` member 400s before the lookup, and the
/// lookup only sees epic-typed rows.
#[allow(clippy::too_many_arguments)]
async fn partial_update_inner(
    state: AppState,
    pool: sqlx::PgPool,
    mut tenant: CoreTenant,
    user_id: Uuid,
    slug: String,
    pk: Uuid,
    req: axum::extract::Request,
    epic: bool,
) -> HandlerResult {
    let (parts, bytes) = split_core_parts(req).await?;
    let mut data = match negotiate_input(&parts.headers, &bytes) {
        Ok(data) => data,
        Err(response) => return Ok(response),
    };
    // `request.data.pop(...)` runs before the lookup: on a non-dict body
    // the pop itself raises (`TypeError`/`AttributeError`) → Django's 500,
    // regardless of whether the issue exists.
    // The epics collection locks the type (`epics.py:188-192`): the
    // membership test runs on the parsed body before the lookup, exactly
    // Python's `in` (key for dicts, element for lists, substring for
    // strings; scalars raise → 500).
    if epic && epic_type_member(&data)? {
        return Ok(type_locked_response());
    }
    if data.value.as_object().is_none() {
        return Err(Denial::ServerError);
    }
    // `request.data.pop("skip_activity", False)` mutates the parsed body —
    // both dumps below exclude the key.
    let skip_activity = data.pop_skip_activity();
    let is_description_update = data.get_is_set("description_html");
    // The epics collection only sees epic-typed rows: a live row
    // outside the boundary 404s exactly Django's `get_object` miss.
    if epic && !epic_exists(&pool, &slug, &tenant.project_id, &pk).await? {
        return Ok(not_found_detail(
            ISSUE_NOT_FOUND_BODY,
            StatusCode::NOT_FOUND,
        ));
    }
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
    let requested = domain_issues::to_spaced_json(&data.value).ok_or(Denial::ServerError)?;
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
    let prev = crate::issue_transition::capture_prior_state_pool(&pool, Some(pk))
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut row = looked_up.save;
    apply_validated_to_existing(&mut row, &validated, &filtered)?;
    if row.state_id.is_none() {
        // The NULL-state leg's `self.project` touch (a miss is the
        // required-object 404), ahead of the domain write.
        touch_save_project(&pool, &row.project_id).await?;
    }
    // The m2m plan resolves pre-save; the legs run inside the domain
    // write, ahead of the row write (`update()` pops them first).
    let (assignee_present, assignees) =
        v_m2m_effective(filtered.assignee_ids.clone(), &validated, "assignee_ids");
    let (label_present, labels) =
        v_m2m_effective(filtered.label_ids.clone(), &validated, "label_ids");
    // `update()` assigns `updated_at`, then `save()`'s `auto_now`
    // overwrites it — the net effect is one save-time stamp.
    // (`updated_by` is stamped from the write context.)
    row.updated_at = utc_now_micros();
    // `base_host` raises past the fire: pure over settings, resolved here,
    // effective in the chain ahead of the activity trio.
    let origin = request_origin(&state).ok();
    let dispatch_immediate = !skip_immediate_dispatch(&parts.headers);
    // The three task enqueues are skipped for description-only migration
    // skips; the save + fire always run.
    let activity = if skip_activity && is_description_update {
        None
    } else {
        Some(domain_issues::UpdateActivity {
            origin: origin.clone(),
            requested: requested.clone(),
            current_instance: current_instance.clone(),
            raw_body: data.value.clone(),
            slug: slug.clone(),
            notification: Some(true),
            require_origin: true,
            emit_version: true,
        })
    };
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = write_ctx(&tenant, &user_id);
    let transition: Arc<dyn IssueTransitionHook> = Arc::new(AppTransitionHook::new(&state, &pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pool));
    let outcome = domain::run(&ctx, orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let pool = pool.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            domain_issues::partial_update(
                &ctx,
                txn,
                &pool,
                domain_issues::PartialIssue {
                    // The overlay never moves the audit pair (verified at
                    // the ORM-03/B migration), so the post-overlay row
                    // still carries the pre-save values Django reads.
                    m2m_audit: (row.created_by_id, row.updated_by_id),
                    sequence: None,
                    fields: row,
                    assignees: assignee_present.then_some(assignees),
                    labels: label_present.then_some(labels),
                    prev_state: prev,
                    fire: domain_issues::FireSpec {
                        dispatch_immediate,
                        created_by: Some(user_id),
                        moved_by_run: None,
                        now: utc_now_micros(),
                    },
                    activity,
                },
            )
            .await
        })
    })
    .await
    .map_err(domain_denial)?;
    if outcome.chain.stopped().is_some() {
        return Err(Denial::ServerError);
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
    let instance = write_instance_of_save(&looked_up);
    let filtered =
        match run_validate(&pool, &state, &validated, None, Some(&instance), &tenant).await {
            Ok(filtered) => filtered,
            Err(response) => return Ok(response),
        };
    let prev = crate::issue_transition::capture_prior_state_pool(&pool, Some(pk))
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut row = looked_up;
    apply_validated_to_existing(&mut row, &validated, &filtered)?;
    if row.state_id.is_none() {
        // The NULL-state leg's `self.project` touch, ahead of the write.
        touch_save_project(&pool, &row.project_id).await?;
    }
    // The m2m plan resolves pre-save; the legs run inside the domain
    // write, ahead of the row write (`update()` pops them first).
    let (assignee_present, assignees) =
        v_m2m_effective(filtered.assignee_ids.clone(), &validated, "assignee_ids");
    let (label_present, labels) =
        v_m2m_effective(filtered.label_ids.clone(), &validated, "label_ids");
    // `update()` assigns `updated_at`, then `save()`'s `auto_now`
    // overwrites it — the net effect is one save-time stamp.
    row.updated_at = utc_now_micros();
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = write_ctx(&tenant, &user_id);
    let transition: Arc<dyn IssueTransitionHook> = Arc::new(AppTransitionHook::new(&state, &pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pool));
    let outcome = domain::run(&ctx, orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let pool = pool.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            domain_issues::update(
                &ctx,
                txn,
                &pool,
                domain_issues::UpdateIssue {
                    m2m_audit: (row.created_by_id, row.updated_by_id),
                    fields: row,
                    assignees: assignee_present.then_some(assignees),
                    labels: label_present.then_some(labels),
                    prev_state: prev,
                    fire: domain_issues::FireSpec {
                        dispatch_immediate: true,
                        created_by: Some(user_id),
                        moved_by_run: None,
                        now: utc_now_micros(),
                    },
                },
            )
            .await
        })
    })
    .await
    .map_err(domain_denial)?;
    if outcome.chain.stopped().is_some() {
        return Err(Denial::ServerError);
    }
    // The representation renders from the saved row (hook outputs
    // included), as it rendered from the post-legs row before.
    let row = IssueSaveRow::from(&outcome.issue);
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
    destroy_issue_inner(state, pool, tenant, user_id, slug, pk).await
}

/// The `DELETE` pipeline after the gate: the destroy-scoped lookup, the
/// synced guard, soft-delete + m2m clears + member pruning + subscribers,
/// and the deleting `model_activity`.
async fn destroy_issue_inner(
    state: AppState,
    pool: sqlx::PgPool,
    tenant: CoreTenant,
    user_id: Uuid,
    slug: String,
    pk: Uuid,
) -> HandlerResult {
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
    let prev = crate::issue_transition::capture_prior_state_pool(&pool, Some(pk))
        .await
        .map_err(|_| Denial::ServerError)?;
    // `issue.delete()`: `deleted_at` now, then the full `save()` (the
    // state legs run — the FK follow resolves triage/deleted states,
    // only a dangling FK 404s — plus stripped + audit), then the
    // related-objects forward.
    // `issue.delete()`: `deleted_at` now, then the full `save()` (the
    // state legs run — the FK follow resolves triage/deleted states,
    // only a dangling FK 404s — plus stripped + audit), then the
    // related-objects forward, the hard recent-visit delete and the
    // unsubscribed `deleted` activity — as one domain write + chain.
    if row.state_id.is_none() {
        // The NULL-state leg's `self.project` touch, ahead of the write.
        touch_save_project(&pool, &row.project_id).await?;
    }
    row.updated_at = utc_now_micros();
    // `base_host` raises past the visit delete: pure over settings,
    // resolved here, effective in the chain ahead of the activity.
    let origin = request_origin(&state).ok();
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = write_ctx(&tenant, &user_id);
    let transition: Arc<dyn IssueTransitionHook> = Arc::new(AppTransitionHook::new(&state, &pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pool));
    let outcome = domain::run(&ctx, orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let pool = pool.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            domain_issues::soft_delete(
                &ctx,
                txn,
                &pool,
                domain_issues::SoftDeleteIssue {
                    fields: row,
                    prev_state: prev,
                    fire: domain_issues::FireSpec {
                        dispatch_immediate: true,
                        created_by: Some(user_id),
                        moved_by_run: None,
                        now: utc_now_micros(),
                    },
                    origin,
                    slug: slug.clone(),
                    completion: false,
                    visit_delete: true,
                    activity_current: Value::Object(Map::new()),
                    activity_notification: Some(true),
                    activity_require_origin: true,
                    activity_subscriber: Some(false),
                    skip_activity: false,
                },
            )
            .await
        })
    })
    .await
    .map_err(domain_denial)?;
    if outcome.chain.stopped().is_some() {
        return Err(Denial::ServerError);
    }
    Ok(empty_response())
}

/// `GET .../issues/<pk>/` (`base.py:486-620`): ADMIN/MEMBER/GUEST/creator
/// gate, the slug-scoped project fetch, the live-only annotated lookup,
/// the guest-view rule, the visit enqueue (which fires even when the
/// `sub_issues` expansion 500s below it), and the 37-key detail body.
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
    let guest: Option<Uuid> = project_members::Entity::find()
        .select_only()
        .column(project_members::Column::Id)
        .join(
            JoinType::InnerJoin,
            project_members::Relation::Workspaces.def(),
        )
        .filter(project_members::Column::MemberId.eq(user_id))
        .filter(project_members::Column::ProjectId.eq(tenant.project_id))
        .filter(workspaces::Column::Slug.eq(slug.as_str()))
        .filter(project_members::Column::Role.eq(ROLE_GUEST as i16))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&orm(&pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    if guest.is_some()
        && !tenant.guest_view_all_features
        && looked_up.save.created_by_id != Some(user_id)
    {
        return Ok(not_found_detail(GUEST_VIEW_BODY, StatusCode::FORBIDDEN));
    }
    enqueue_recent_visited(&pool, &slug, &pk.to_string(), &user_id, &tenant.project_id).await;
    // `?expand=` runs `DynamicBaseSerializer.to_representation`: mapper names
    // nest, declared names clobber, `issue_attachments` appends, and a mapped
    // name that is not an `Issue` attribute (`sub_issues`, `user`, …) raises
    // `AttributeError` → the generic 500 (the visit enqueue above already
    // fired). Ported as the shared kernel, bugs included.
    let expand_raw = parts.uri.query().and_then(|query| {
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .filter_map(|(key, value)| (key == "expand").then_some(value))
            .next_back()
    });
    let mut body = render_detail_map(&pool, &looked_up, &tenant.timezone, true, true).await?;
    if let Some(expands) = super::expand::split_expand(expand_raw) {
        let anchor = super::expand::ExpandAnchor {
            issue_id: looked_up.save.id,
            workspace_id: looked_up.save.workspace_id,
            project_id: looked_up.save.project_id,
            created_by_id: looked_up.save.created_by_id,
            updated_by_id: looked_up.save.updated_by_id,
            state_id: looked_up.save.state_id,
            parent_id: looked_up.save.parent_id,
            estimate_point_id: looked_up.save.estimate_point_id,
        };
        super::expand::apply_dynamic_expand(
            &pool,
            &anchor,
            &tenant.timezone,
            super::expand::DETAIL_EXPAND_FIELDS,
            &expands,
            &mut body,
        )
        .await?;
    }
    Ok(json_response(
        serde_json::to_string(&Value::Object(body)).map_err(|_| Denial::ServerError)?,
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
        soft_delete_issue_links(&pool, &survivors).await?;
        // The issues leg reuses the `issue_objects` scope (it soft-deletes
        // only in-scope rows; out-of-scope ids still count as misses).
        // The issues leg reuses the `issue_objects` scope via the
        // domain bulk write (queryset `delete()`: one scoped UPDATE,
        // no `save()`, no fire, no activity).
        domain_issues::bulk_soft_delete_issues(&pool, &survivors, &tenant.project_id, &slug)
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

/// The cycle / module link soft-deletes (`CycleIssue` / `ModuleIssue`
/// queryset `.delete()`s): live links of the swept ids. Each `.delete()`
/// evaluates its own `now()`.
async fn soft_delete_issue_links(pool: &sqlx::PgPool, survivors: &[Uuid]) -> Result<(), Denial> {
    let db = orm(pool);
    cycle_issues::Entity::update_many()
        .col_expr(
            cycle_issues::Column::DeletedAt,
            Expr::value(utc_now_micros()),
        )
        .filter(cycle_issues::Column::DeletedAt.is_null())
        .filter(cycle_issues::Column::IssueId.is_in(survivors.iter().copied()))
        .exec(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    module_issues::Entity::update_many()
        .col_expr(
            module_issues::Column::DeletedAt,
            Expr::value(utc_now_micros()),
        )
        .filter(module_issues::Column::DeletedAt.is_null())
        .filter(module_issues::Column::IssueId.is_in(survivors.iter().copied()))
        .exec(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `len(issues)`: the `issue_objects`-scoped count behind the message.
async fn count_bulk_issues(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    survivors: &[Uuid],
) -> Result<i64, Denial> {
    // An empty survivor list still runs the scoped query (Django's
    // `pk__in=[]` short-circuits to an empty result; the count is 0).
    let total = issues::Entity::find()
        .join(JoinType::InnerJoin, issues::Relation::Workspaces.def())
        .join(JoinType::LeftJoin, issues::Relation::States.def())
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .filter(issues::Column::DeletedAt.is_null())
        .filter(issues::Column::ProjectId.eq(*project_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(issue_objects_condition())
        .filter(issues::Column::Id.is_in(survivors.iter().copied()))
        .count(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(total as i64)
}

// ---------------------------------------------------------------------------
// Epics (`app/views/issue/epics.py`, NEWFRONT-984)
// ---------------------------------------------------------------------------

/// Collection path in `app/urls/issue.py:130` form.
pub const EPIC_COLLECTION_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/epics/";
/// Detail path in `app/urls/issue.py:134-138` form.
pub const EPIC_DETAIL_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/epics/{pk}/";

/// The no-epic-type refusal (`epics.py:76-80`).
const NO_EPIC_TYPE_BODY: &str =
    r#"{"error":"This project has no epic type, so no epic can be created."}"#;
/// The locked-type refusal (`epics.py:189-192`).
const TYPE_LOCKED_BODY: &str =
    r#"{"error":"An epic's type cannot change through the epics collection."}"#;

/// `resolve_epic_type` (`epics.py:47-61`): the project's epic type id —
/// lowest link level — or `None`. The link must be live and the type live,
/// epic, and active; no workspace or default filter.
async fn resolve_epic_type(pool: &sqlx::PgPool, project_id: &Uuid) -> Result<Option<Uuid>, Denial> {
    project_issue_types::Entity::find()
        .select_only()
        .column(issue_types::Column::Id)
        .join(
            JoinType::InnerJoin,
            project_issue_types::Relation::IssueTypes.def(),
        )
        .filter(project_issue_types::Column::ProjectId.eq(*project_id))
        .filter(project_issue_types::Column::DeletedAt.is_null())
        .filter(issue_types::Column::IsEpic.eq(true))
        .filter(issue_types::Column::IsActive.eq(true))
        .filter(issue_types::Column::DeletedAt.is_null())
        .order_by_asc(project_issue_types::Column::Level)
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)
}

/// The epic boundary (`epics.py:65-66`): the row
/// exists in the `issue_objects` scope for this project/slug AND is
/// epic-typed. Django's `filter(type__is_epic=True)` inner-joins with no
/// related-manager guard, so untyped rows fall outside while rows under
/// inactive or soft-deleted epic types still match.
async fn epic_exists(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    pk: &Uuid,
) -> Result<bool, Denial> {
    let to_type: sea_orm::RelationDef = issues::Entity::belongs_to(issue_types::Entity)
        .from(issues::Column::TypeId)
        .to(issue_types::Column::Id)
        .into();
    let row: Option<Uuid> = scoped_issue_select(slug, *project_id, *pk, IssueScope::IssueObjects)
        .select_only()
        .column(issues::Column::Id)
        .join(JoinType::InnerJoin, to_type)
        .filter(issue_types::Column::IsEpic.eq(true))
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// Python's `"type" in request.data` (`epics.py:188`): key for dicts
/// (uploads merged, like DRF), element for lists, substring for strings,
/// false for null; scalars raise → the generic 500.
fn epic_type_member(data: &RequestData) -> Result<bool, Denial> {
    match &data.value {
        Value::Object(_) => Ok(data.contains("type")),
        Value::Array(items) => Ok(items.iter().any(|item| item.as_str() == Some("type"))),
        Value::String(text) => Ok(text.contains("type")),
        Value::Null => Ok(false),
        Value::Number(_) | Value::Bool(_) => Err(Denial::ServerError),
    }
}

fn type_locked_response() -> Response {
    json_bad_request(TYPE_LOCKED_BODY)
}

/// `POST .../epics/` (`epics.py:73-95`): the issues gate, the
/// resolve-then-force type leg, then the shared create pipeline. The
/// response is the same 27-key `.values()` body as the plain create.
pub async fn epic_create(
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
    let Some(epic_type) = resolve_epic_type(&pool, &tenant.project_id).await? else {
        return Ok(json_bad_request(NO_EPIC_TYPE_BODY));
    };
    create_issue_inner(state, pool, tenant, user_id, slug, req, Some(epic_type)).await
}

/// `GET .../epics/<pk>/` (`epics.py:172-184`): the issues gate, the epic
/// boundary, then the shared retrieve pipeline. `is_epic` stamps `True`
/// on 200s (a no-op for in-boundary rows — the renderer already reads
/// the type flag — but exact when it matters).
pub async fn epic_retrieve(
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
    let user_id = core_session(extension.clone())?;
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
    if !epic_exists(&pool, &slug, &tenant.project_id, &pk).await? {
        return Ok(not_found_detail(NOT_FOUND_BODY, StatusCode::NOT_FOUND));
    }
    let response = retrieve_issue(
        State(state),
        Path((slug, project_raw, pk_raw)),
        extension,
        req,
    )
    .await?;
    if response.status() != StatusCode::OK {
        return Ok(response);
    }
    let (mut parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut rendered: Value = serde_json::from_slice(&bytes).map_err(|_| Denial::ServerError)?;
    if let Some(map) = rendered.as_object_mut() {
        map.insert("is_epic".to_owned(), Value::Bool(true));
    }
    let stamped = serde_json::to_string(&rendered).map_err(|_| Denial::ServerError)?;
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    Ok(Response::from_parts(parts, axum::body::Body::from(stamped)))
}

/// `PATCH .../epics/<pk>/` (`epics.py:186-193`): the issues gate, then the
/// shared patch pipeline in epic mode (locked `type`, epic-scoped
/// lookup).
pub async fn epic_partial_update(
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
    let tenant = core_gate(
        &pool,
        &slug,
        &project_raw,
        &user_id,
        &GATE_PATCH,
        Some(pk),
        ProjectCheck::None,
    )
    .await?;
    partial_update_inner(state, pool, tenant, user_id, slug, pk, req, true).await
}

/// `DELETE .../epics/<pk>/` (`epics.py:195-202`): the issues gate, the
/// epic boundary, then the shared destroy pipeline.
pub async fn epic_destroy(
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
    if !epic_exists(&pool, &slug, &tenant.project_id, &pk).await? {
        return Ok(not_found_detail(NOT_FOUND_BODY, StatusCode::NOT_FOUND));
    }
    destroy_issue_inner(state, pool, tenant, user_id, slug, pk).await
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
        // Valid shapes still parse, including multibyte separators
        // (`fromisoformat` accepts any single char there; the
        // `split_date_time`/`parse_iso_time` vectors moved to
        // `pidash_compat::datetime` with those functions).
        assert!(parse_drf_date("2024W05").is_some());
        assert!(parse_drf_date("2024W053").is_some());
        assert!(parse_drf_date("2024-W05-3").is_some());
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

    /// The lookup statement keeps the `issue_objects` joins and scope, the
    /// slug filter and the `-created_at` order.
    #[test]
    fn scoped_issue_select_shape() {
        use sea_orm::QueryTrait;
        let id = Uuid::nil();
        let live = scoped_issue_select("acme", id, id, IssueScope::Live)
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert!(live.contains(r#"INNER JOIN "workspaces""#), "{live}");
        assert!(!live.contains(r#""states""#), "{live}");
        assert!(live.contains(r#""issues"."deleted_at" IS NULL"#), "{live}");
        assert!(
            live.ends_with(r#"ORDER BY "issues"."created_at" DESC"#),
            "{live}"
        );
        let scoped = scoped_issue_select("acme", id, id, IssueScope::IssueObjects)
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert!(scoped.contains(r#"LEFT JOIN "states""#), "{scoped}");
        assert!(scoped.contains(r#"INNER JOIN "projects""#), "{scoped}");
        assert!(
            scoped
                .contains(r#"NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL)"#),
            "{scoped}"
        );
        assert!(
            scoped.contains(r#""projects"."archived_at" IS NULL"#),
            "{scoped}"
        );
        assert!(
            scoped.contains(r#""issues"."is_draft" = FALSE"#),
            "{scoped}"
        );
    }

    /// Retrieve / PATCH / create each compile to ONE statement: the
    /// annotations ride as correlated subqueries on the outer issue id.
    #[test]
    fn annotated_lookups_are_one_statement() {
        use sea_orm::QueryTrait;
        let id = Uuid::nil();
        let retrieve = annotated_issue_select("acme", id, id, id, false)
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        for needle in [
            r#"(SELECT "cycle_issues"."cycle_id" FROM "cycle_issues" WHERE "cycle_issues"."issue_id" = "issues"."id""#,
            r#"(SELECT COUNT(*) FROM "issue_links" WHERE "issue_links"."issue_id" = "issues"."id""#,
            r#"(SELECT COUNT(*) FROM "issues" AS "c" LEFT JOIN "states" AS "cs""#,
            r#"(SELECT ARRAY_AGG(DISTINCT "issue_labels"."label_id") FROM "issue_labels""#,
            r#"INNER JOIN "project_members" ON "project_members"."member_id" = "users"."id""#,
            r#"(SELECT ARRAY_AGG(DISTINCT "module_issues"."module_id") FROM "module_issues""#,
            r#"EXISTS(SELECT 1 FROM "issue_subscribers" INNER JOIN "workspaces" AS "sw""#,
            r#""states"."group" AS "state_group""#,
            r#"LEFT JOIN "states" ON"#,
        ] {
            assert!(retrieve.contains(needle), "{needle}\n{retrieve}");
        }
        let patch = annotated_issue_select("acme", id, id, id, true)
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert!(!patch.contains("is_subscribed"), "{patch}");
        assert_eq!(
            patch.matches(r#"LEFT JOIN "states" ON"#).count(),
            1,
            "{patch}"
        );
        let create = annotate_issue_select(
            scoped_issue_select("acme", id, id, IssueScope::IssueObjects),
            AssigneeLeg::Plain,
        )
        .build(sea_orm::DbBackend::Postgres)
        .to_string();
        assert!(!create.contains("project_members"), "{create}");
        assert!(create.contains(r#"AS "assignee_ids""#), "{create}");
    }

    /// The core reads against constraint-free temp tables on a
    /// one-connection pool: gates, tenant facts, field and validate
    /// probes, scoped lookups, annotations, detail legs, the PUT m2m read,
    /// the bulk count + link soft-deletes, and the epic reads.
    #[tokio::test]
    async fn live_core_queries() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        for table in [
            "users",
            "workspaces",
            "workspace_members",
            "projects",
            "project_members",
            "issues",
            "states",
            "cycle_issues",
            "issue_links",
            "file_assets",
            "issue_labels",
            "issue_assignees",
            "module_issues",
            "modules",
            "issue_subscribers",
            "issue_reactions",
            "labels",
            "estimate_points",
            "issue_types",
            "project_issue_types",
            "pod",
            "runner",
            "agent_run",
            "assistant_user_llm_config",
            "issue_agent_ticker",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        let id = Uuid::new_v4;
        let (ws, project, gone_project) = (id(), id(), id());
        let (admin, member, guest, stranger) = (id(), id(), id(), id());
        let (state, triage) = (id(), id());
        let (open, child, triage_child, draft, archived, deleted, epic) =
            (id(), id(), id(), id(), id(), id(), id());
        let (epic_type, plain_type, label_a, label_b, gone_label) = (id(), id(), id(), id(), id());
        let (module_live, module_archived, cycle, estimate, the_pod) =
            (id(), id(), id(), id(), id());
        let issue = |id: Uuid,
                     state: Option<Uuid>,
                     parent: Option<Uuid>,
                     type_id: Option<Uuid>,
                     extra: &str| {
            let opt = |value: Option<Uuid>| value.map_or("NULL".to_owned(), |v| format!("'{v}'"));
            let (archived, draft, deleted) = match extra {
                "archived" => ("current_date", "false", "NULL"),
                "draft" => ("NULL", "true", "NULL"),
                "deleted" => ("NULL", "false", "now()"),
                _ => ("NULL", "false", "NULL"),
            };
            format!(
                "INSERT INTO issues (id, name, workspace_id, project_id, state_id, parent_id, type_id, \
                 sort_order, priority, complexity_score, sequence_id, created_at, updated_at, created_by_id, \
                 is_draft, archived_at, deleted_at, description_html, description_json, git_work_branch, workpad) \
                 VALUES ('{id}', 'n', '{ws}', '{project}', {}, {}, {}, 65535, 'none', 0, 7, \
                 '2026-01-02T03:04:05.123456Z', '2026-01-02T03:04:05Z', '{member}', {draft}, {archived}, {deleted}, \
                 '<p></p>', '{{}}', '', '')",
                opt(state),
                opt(parent),
                opt(type_id),
            )
        };
        for sql in [
            format!(
                "INSERT INTO users (id, user_timezone, is_active, is_bot, created_at) VALUES \
                 ('{admin}', 'UTC', true, false, now() - interval '3 min'), \
                 ('{member}', 'Asia/Tokyo', true, false, now() - interval '2 min'), \
                 ('{guest}', 'UTC', true, true, now() - interval '1 min')"
            ),
            format!("INSERT INTO workspaces (id, slug) VALUES ('{ws}', 'acme')"),
            format!(
                "INSERT INTO workspace_members (id, workspace_id, member_id, role, is_active) VALUES \
                 (gen_random_uuid(), '{ws}', '{admin}', 20, true), \
                 (gen_random_uuid(), '{ws}', '{member}', 15, true)"
            ),
            format!(
                "INSERT INTO projects (id, workspace_id, default_assignee_id, guest_view_all_features, \
                 archived_at, deleted_at, created_at, agent_ticking_enabled, agent_default_max_ticks, \
                 agent_default_interval_seconds, agent_review_default_interval_seconds, \
                 agent_test_default_interval_seconds) VALUES \
                 ('{project}', '{ws}', '{member}', false, NULL, NULL, now(), true, 5, 60, 70, 80), \
                 ('{gone_project}', '{ws}', NULL, true, NULL, now(), now(), true, 5, 60, 70, 80)"
            ),
            format!(
                "INSERT INTO project_members (id, workspace_id, project_id, member_id, role, is_active, deleted_at, created_at) VALUES \
                 (gen_random_uuid(), '{ws}', '{project}', '{admin}', 20, true, NULL, now() - interval '3 min'), \
                 (gen_random_uuid(), '{ws}', '{project}', '{member}', 15, true, NULL, now() - interval '2 min'), \
                 (gen_random_uuid(), '{ws}', '{project}', '{guest}', 5, true, NULL, now() - interval '1 min'), \
                 (gen_random_uuid(), '{ws}', '{project}', '{stranger}', 20, true, now(), now())"
            ),
            format!(
                "INSERT INTO states (id, name, \"group\", project_id, workspace_id, deleted_at) VALUES \
                 ('{state}', 'Todo', 'unstarted', '{project}', '{ws}', NULL), \
                 ('{triage}', 'Triage', 'triage', '{project}', '{ws}', NULL)"
            ),
            issue(open, Some(state), None, Some(plain_type), ""),
            issue(child, None, Some(open), None, ""),
            issue(triage_child, Some(triage), Some(open), None, ""),
            issue(draft, Some(state), Some(open), None, "draft"),
            issue(archived, Some(state), None, None, "archived"),
            issue(deleted, Some(state), None, None, "deleted"),
            issue(epic, Some(state), None, Some(epic_type), ""),
            format!(
                "INSERT INTO cycle_issues (id, cycle_id, issue_id, deleted_at) VALUES \
                 (gen_random_uuid(), '{cycle}', '{open}', NULL)"
            ),
            format!(
                "INSERT INTO issue_links (id, issue_id, deleted_at) VALUES \
                 (gen_random_uuid(), '{open}', NULL), (gen_random_uuid(), '{open}', NULL), \
                 (gen_random_uuid(), '{open}', now())"
            ),
            format!(
                "INSERT INTO file_assets (id, issue_id, entity_type, deleted_at) VALUES \
                 (gen_random_uuid(), '{open}', 'ISSUE_DESCRIPTION', NULL), \
                 (gen_random_uuid(), '{open}', 'ISSUE_ATTACHMENT', now())"
            ),
            format!(
                "INSERT INTO labels (id, project_id, deleted_at, created_at) VALUES \
                 ('{label_a}', '{project}', NULL, now() - interval '2 min'), \
                 ('{label_b}', '{project}', NULL, now() - interval '1 min'), \
                 ('{gone_label}', '{project}', now(), now())"
            ),
            format!(
                "INSERT INTO issue_labels (id, issue_id, label_id, deleted_at) VALUES \
                 (gen_random_uuid(), '{open}', '{label_b}', NULL), \
                 (gen_random_uuid(), '{open}', '{label_a}', NULL), \
                 (gen_random_uuid(), '{open}', '{label_a}', NULL), \
                 (gen_random_uuid(), '{open}', '{gone_label}', now())"
            ),
            // `member` holds two active memberships (the join is not
            // project-scoped), `guest` is a bot but active, `stranger` has
            // only a soft-deleted (still active) membership.
            format!(
                "INSERT INTO issue_assignees (id, issue_id, assignee_id, deleted_at) VALUES \
                 (gen_random_uuid(), '{open}', '{member}', NULL), \
                 (gen_random_uuid(), '{open}', '{admin}', NULL), \
                 (gen_random_uuid(), '{open}', '{guest}', now()), \
                 (gen_random_uuid(), '{open}', '{guest}', NULL)"
            ),
            format!(
                "INSERT INTO modules (id, archived_at) VALUES ('{module_live}', NULL), ('{module_archived}', now())"
            ),
            format!(
                "INSERT INTO module_issues (id, issue_id, module_id, deleted_at) VALUES \
                 (gen_random_uuid(), '{open}', '{module_live}', NULL), \
                 (gen_random_uuid(), '{open}', '{module_archived}', NULL)"
            ),
            format!(
                "INSERT INTO issue_subscribers (id, workspace_id, project_id, issue_id, subscriber_id, deleted_at) VALUES \
                 (gen_random_uuid(), '{ws}', '{project}', '{open}', '{member}', NULL), \
                 (gen_random_uuid(), '{ws}', '{project}', '{open}', '{admin}', now())"
            ),
            format!(
                "INSERT INTO issue_reactions (id, issue_id, deleted_at) VALUES (gen_random_uuid(), '{open}', NULL)"
            ),
            format!(
                "INSERT INTO estimate_points (id, project_id, deleted_at) VALUES ('{estimate}', '{project}', NULL)"
            ),
            format!(
                "INSERT INTO issue_types (id, is_epic, is_active, deleted_at) VALUES \
                 ('{epic_type}', true, true, NULL), ('{plain_type}', false, true, NULL)"
            ),
            format!(
                "INSERT INTO project_issue_types (id, project_id, issue_type_id, level, deleted_at) VALUES \
                 (gen_random_uuid(), '{project}', '{plain_type}', 0, NULL), \
                 (gen_random_uuid(), '{project}', '{epic_type}', 1, NULL)"
            ),
            format!(
                "INSERT INTO pod (id, project_id, deleted_at) VALUES ('{the_pod}', '{project}', now())"
            ),
            format!(
                "INSERT INTO runner (id, owner_id, workspace_id, pod_id, provisioning, pod_dispatch, status, \
                 last_heartbeat_at, revoked_at, paused_at) VALUES \
                 (gen_random_uuid(), '{member}', '{ws}', '{the_pod}', 'desktop_bundled', 'pinned_only', 'online', now(), NULL, NULL), \
                 (gen_random_uuid(), '{admin}', '{ws}', '{the_pod}', 'desktop_bundled', 'pinned_only', 'online', now() - interval '1 hour', NULL, now())"
            ),
            format!(
                "INSERT INTO agent_run (id, work_item_id, status) VALUES \
                 (gen_random_uuid(), '{open}', 'completed'), (gen_random_uuid(), '{epic}', 'running')"
            ),
            format!(
                "INSERT INTO assistant_user_llm_config (id, user_id, api_key_encrypted) VALUES \
                 (1, '{member}', '\\x01'), (2, '{admin}', '')"
            ),
            format!(
                "INSERT INTO issue_agent_ticker (id, created_at, updated_at, issue_id, used, granted, \
                 waited, user_disabled, enabled, disarm_reason, pending_entry, pending_entry_free, \
                 pending_entry_trigger, deleted_at) VALUES \
                 (gen_random_uuid(), now(), now(), '{open}', 1, 0, 0, false, true, '', false, false, '', NULL), \
                 (gen_random_uuid(), now(), now(), '{epic}', 1, 0, 0, false, true, '', false, false, '', now())"
            ),
        ] {
            sqlx::query(&sql).execute(&pool).await.expect("seed");
        }

        // Gate legs + tenant facts.
        let (tz, active, bot) = core_user(&pool, &member).await.expect("user");
        assert_eq!((tz.name(), active, bot), ("Asia/Tokyo", true, false));
        assert!(matches!(
            core_user(&pool, &stranger).await,
            Err(Denial::Unauthorized)
        ));
        let facts = allow_facts(&pool, "acme", &project, &member, &GATE_DESTROY, Some(open))
            .await
            .expect("facts");
        assert!(facts.is_workspace_member && !facts.is_workspace_admin);
        assert!(facts.is_project_member && !facts.has_allowed_project_role);
        assert!(facts.is_creator);
        let facts = allow_facts(&pool, "acme", &project, &stranger, &GATE_CREATE, None)
            .await
            .expect("facts");
        assert!(!facts.is_workspace_member && !facts.is_project_member && !facts.is_creator);
        let facts = allow_facts(&pool, "acme", &project, &admin, &GATE_DESTROY, Some(open))
            .await
            .expect("facts");
        assert!(facts.is_workspace_admin && facts.has_allowed_project_role && !facts.is_creator);
        assert_eq!(
            tenant_facts(&pool, "acme", &project, &member, ProjectCheck::InSlug)
                .await
                .expect("tenant"),
            (project, ws, Some(member), false)
        );
        assert!(matches!(
            tenant_facts(&pool, "other", &project, &member, ProjectCheck::InSlug).await,
            Err(Denial::NotFound)
        ));
        assert!(tenant_facts(
            &pool,
            "other",
            &project,
            &member,
            ProjectCheck::AnyWorkspace
        )
        .await
        .is_ok());
        assert!(matches!(
            tenant_facts(
                &pool,
                "acme",
                &gone_project,
                &member,
                ProjectCheck::AnyWorkspace
            )
            .await,
            Err(Denial::NotFound)
        ));

        // Field probes.
        assert!(pk_exists(&pool, PkQueryset::StatesAll, &triage)
            .await
            .expect("pk"));
        assert!(!pk_exists(&pool, PkQueryset::States, &triage)
            .await
            .expect("pk"));
        assert!(pk_exists(&pool, PkQueryset::IssuesAll, &deleted)
            .await
            .expect("pk"));
        assert!(!pk_exists(&pool, PkQueryset::Issues, &deleted)
            .await
            .expect("pk"));
        assert!(pk_exists(&pool, PkQueryset::PodsAll, &the_pod)
            .await
            .expect("pk"));
        assert!(!pk_exists(&pool, PkQueryset::Labels, &gone_label)
            .await
            .expect("pk"));
        assert!(pk_exists(&pool, PkQueryset::Users, &guest)
            .await
            .expect("pk"));
        assert!(pk_exists(&pool, PkQueryset::Estimates, &estimate)
            .await
            .expect("pk"));
        assert!(pk_exists(&pool, PkQueryset::Types, &epic_type)
            .await
            .expect("pk"));
        assert_eq!(
            state_row(&pool, PkQueryset::StatesAll, &triage)
                .await
                .expect("state"),
            Some((triage, "triage".to_owned()))
        );
        assert_eq!(
            state_row(&pool, PkQueryset::States, &triage)
                .await
                .expect("state"),
            None
        );

        // `validate()` probes.
        let pod = pod_ref(&pool, &the_pod).await.expect("pod").expect("row");
        assert!(pod.deleted && pod.project_id == project);
        assert!(!has_active_run(&pool, &open).await.expect("run"));
        assert!(has_active_run(&pool, &epic).await.expect("run"));
        assert_eq!(
            filter_assignee_ids(&pool, &project, &[admin, member, guest, stranger])
                .await
                .expect("assignees"),
            vec![member, admin]
        );
        assert_eq!(
            filter_label_ids(&pool, Some(&project), &[label_a, label_b, gone_label])
                .await
                .expect("labels"),
            vec![label_b, label_a]
        );
        assert!(filter_label_ids(&pool, None, &[label_a])
            .await
            .expect("labels")
            .is_empty());
        for (probe, target) in [
            (ScopedProbe::State, state),
            (ScopedProbe::Parent, open),
            (ScopedProbe::Estimate, estimate),
        ] {
            assert!(project_scoped_exists(&pool, probe, Some(&project), &target)
                .await
                .expect("scoped"));
            assert!(!project_scoped_exists(&pool, probe, None, &target)
                .await
                .expect("scoped"));
        }
        assert!(
            !project_scoped_exists(&pool, ScopedProbe::State, Some(&project), &triage)
                .await
                .expect("scoped")
        );
        assert!(assistant_has_api_key(&pool, &member).await.expect("key"));
        assert!(!assistant_has_api_key(&pool, &admin).await.expect("key"));
        assert!(!assistant_has_api_key(&pool, &guest).await.expect("key"));
        assert!(fetch_project_ws(&pool, &project)
            .await
            .expect("project")
            .is_some());
        assert!(fetch_project_ws(&pool, &gone_project)
            .await
            .expect("project")
            .is_none());
        assert!(enrolled_runner(&pool, &member, &project, &ws)
            .await
            .expect("runner"));
        assert!(online_runner(&pool, &member, &project, &ws)
            .await
            .expect("runner"));
        assert!(!paused_runner(&pool, &member, &project, &ws)
            .await
            .expect("runner"));
        assert!(!online_runner(&pool, &admin, &project, &ws)
            .await
            .expect("runner"));
        assert!(paused_runner(&pool, &admin, &project, &ws)
            .await
            .expect("runner"));
        assert!(!enrolled_runner(&pool, &member, &gone_project, &ws)
            .await
            .expect("runner"));
        touch_save_project(&pool, &project).await.expect("project");
        assert!(matches!(
            touch_save_project(&pool, &gone_project).await,
            Err(Denial::NotFound)
        ));

        // Scoped lookups + annotations.
        let row = fetch_annotated_issue(&pool, "acme", &project, &open, &member, false)
            .await
            .expect("retrieve")
            .expect("row");
        assert_eq!(row.save.id, open);
        assert_eq!(row.cycle_id, Some(cycle));
        assert_eq!((row.link_count, row.attachment_count), (Some(2), None));
        // `child` counts; the triage child and the draft do not.
        assert_eq!(row.sub_issues_count, Some(1));
        let mut labels = vec![label_a, label_b];
        labels.sort();
        assert_eq!(row.label_ids, labels);
        let mut assignees = vec![admin, member, guest];
        assignees.sort();
        assert_eq!(row.assignee_ids, assignees);
        assert_eq!(row.module_ids, vec![module_live]);
        assert!(row.is_subscribed);
        assert_eq!(
            (row.state_group.as_deref(), row.state_name.as_deref()),
            (Some("unstarted"), Some("Todo"))
        );
        let row = fetch_annotated_issue(&pool, "acme", &project, &open, &admin, false)
            .await
            .expect("retrieve")
            .expect("row");
        assert!(!row.is_subscribed);
        let row = fetch_annotated_issue(&pool, "acme", &project, &child, &member, true)
            .await
            .expect("patch")
            .expect("row");
        assert_eq!(
            (
                row.state_group,
                row.link_count,
                row.sub_issues_count,
                row.is_subscribed
            ),
            (None, None, None, false)
        );
        assert!(row.label_ids.is_empty() && row.assignee_ids.is_empty());
        assert!(row.module_ids.is_empty() && row.cycle_id.is_none());
        assert_eq!(row.attachment_count, None);
        // The PATCH shape over the fully-annotated issue: same annotations
        // as retrieve, `is_subscribed` never selected.
        let row = fetch_annotated_issue(&pool, "acme", &project, &open, &member, true)
            .await
            .expect("patch")
            .expect("row");
        assert_eq!(row.cycle_id, Some(cycle));
        assert_eq!(
            (row.link_count, row.attachment_count, row.sub_issues_count),
            (Some(2), None, Some(1))
        );
        assert_eq!(row.label_ids, labels);
        assert_eq!(row.assignee_ids, assignees);
        assert_eq!(row.module_ids, vec![module_live]);
        assert!(!row.is_subscribed);
        assert_eq!(
            (row.state_group.as_deref(), row.state_name.as_deref()),
            (Some("unstarted"), Some("Todo"))
        );
        for hidden in [draft, archived, triage_child] {
            assert!(
                fetch_annotated_issue(&pool, "acme", &project, &hidden, &member, true)
                    .await
                    .expect("patch")
                    .is_none()
            );
            assert!(fetch_put_issue(&pool, "acme", &project, &hidden)
                .await
                .expect("put")
                .is_none());
        }
        assert!(
            fetch_annotated_issue(&pool, "acme", &project, &archived, &member, false)
                .await
                .expect("retrieve")
                .is_some()
        );
        assert!(fetch_destroy_issue(&pool, "acme", &project, &deleted)
            .await
            .expect("destroy")
            .is_none());
        assert!(fetch_destroy_issue(&pool, "other", &project, &open)
            .await
            .expect("destroy")
            .is_none());
        let put = fetch_put_issue(&pool, "acme", &project, &open)
            .await
            .expect("put")
            .expect("row");
        assert_eq!((put.sequence_id, put.priority.as_str()), (7, "none"));

        // Create response: plain assignees (no member join).
        let post = fetch_create_response_row(&pool, "acme", &project, &open)
            .await
            .expect("post")
            .expect("row");
        assert_eq!(post.0.id, open);
        assert_eq!(post.1.cycle_id, Some(cycle));
        assert_eq!(
            (
                post.1.link_count,
                post.1.attachment_count,
                post.1.sub_issues_count
            ),
            (Some(2), None, Some(1))
        );
        assert_eq!(post.1.label_ids, labels);
        assert_eq!(post.1.assignee_ids, assignees);
        assert_eq!(post.1.module_ids, vec![module_live]);
        let empty = fetch_create_response_row(&pool, "acme", &project, &child)
            .await
            .expect("post")
            .expect("row");
        assert_eq!(
            (
                empty.1.cycle_id,
                empty.1.link_count,
                empty.1.attachment_count,
                empty.1.sub_issues_count
            ),
            (None, None, None, None)
        );
        assert!(
            empty.1.label_ids.is_empty()
                && empty.1.assignee_ids.is_empty()
                && empty.1.module_ids.is_empty()
        );
        let body = render_create_response_body(&post, &chrono_tz::UTC).expect("body");
        assert!(
            body.starts_with(&format!("{{\"id\":\"{open}\",\"name\":\"n\",\"state_id\":\"{state}\",\"sort_order\":65535.0,")),
            "{body}"
        );
        assert!(
            body.contains(
                "\"created_at\":\"2026-01-02T03:04:05.123456Z\",\"updated_at\":\"2026-01-02T03:04:05Z\""
            ),
            "{body}"
        );
        assert!(
            body.contains(&format!(
                "\"deleted_at\":null,\"cycle_id\":\"{cycle}\",\"link_count\":2,\"attachment_count\":null,\"sub_issues_count\":1,"
            )),
            "{body}"
        );
        assert!(fetch_create_response_row(&pool, "acme", &project, &draft)
            .await
            .expect("post")
            .is_none());

        // Detail legs.
        touch_detail_prefetches(&pool, &open)
            .await
            .expect("prefetch");
        assert_eq!(
            fetch_ticker(&pool, &open)
                .await
                .expect("ticker")
                .map(|t| t.issue_id),
            Some(open)
        );
        assert!(fetch_ticker(&pool, &epic).await.expect("ticker").is_none());
        let policy = fetch_clock_policy(&pool, &project).await.expect("policy");
        assert_eq!(
            (
                policy.agent_ticking_enabled,
                policy.agent_default_max_ticks,
                policy.agent_test_default_interval_seconds
            ),
            (Some(true), Some(5), Some(80))
        );
        assert!(matches!(
            fetch_clock_policy(&pool, &gone_project).await,
            Err(Denial::ServerError)
        ));
        assert!(!fetch_is_epic(&pool, None).await.expect("epic"));
        assert!(!fetch_is_epic(&pool, Some(plain_type)).await.expect("epic"));
        assert!(fetch_is_epic(&pool, Some(epic_type)).await.expect("epic"));

        // PUT m2m: through rows unfiltered (duplicates kept), `-created_at`.
        let (put_assignees, put_labels) = fetch_put_m2m(&pool, &open).await.expect("m2m");
        assert_eq!(put_assignees, vec![guest, guest, member, admin]);
        assert_eq!(put_labels, vec![label_b, label_a, label_a]);

        // Bulk delete legs.
        assert_eq!(
            count_bulk_issues(
                &pool,
                "acme",
                &project,
                &[open, draft, archived, child, id()]
            )
            .await
            .expect("count"),
            2
        );
        assert_eq!(
            count_bulk_issues(&pool, "acme", &project, &[])
                .await
                .expect("count"),
            0
        );

        soft_delete_issue_links(&pool, &[open])
            .await
            .expect("links");
        let live_links: Vec<(String, i64)> = sqlx::query_as(
            "SELECT 'cycle', COUNT(*) FROM cycle_issues WHERE deleted_at IS NULL \
             UNION ALL SELECT 'module', COUNT(*) FROM module_issues WHERE deleted_at IS NULL",
        )
        .fetch_all(&pool)
        .await
        .expect("links");
        assert_eq!(
            live_links,
            vec![("cycle".to_owned(), 0), ("module".to_owned(), 0)]
        );

        // Epics.
        assert_eq!(
            resolve_epic_type(&pool, &project).await.expect("type"),
            Some(epic_type)
        );
        assert!(epic_exists(&pool, "acme", &project, &epic)
            .await
            .expect("epic"));
        assert!(!epic_exists(&pool, "acme", &project, &open)
            .await
            .expect("epic"));
        assert!(!epic_exists(&pool, "acme", &project, &child)
            .await
            .expect("epic"));
    }
}
