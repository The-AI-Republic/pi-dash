//! Scheduler permission gates + feature-flag guard (D-36, stage 5, PIDASHCONV-632).
//!
//! Ports the `@allow_permission` gates on the 5 scheduler routes
//! (`apps/api/pi_dash/app/urls/scheduler.py:16-46`, decorators in
//! `apps/api/pi_dash/app/views/scheduler/views.py` and
//! `apps/api/pi_dash/app/views/scheduler/occurrences.py`) and the
//! `_feature_enabled` kill switch (`views.py:32-40`). Fixture:
//! `rust-api/fixtures/app_scheduler/guards/permissions.golden.json`
//! (F36-09).
//!
//! Shape of the port, following the [`crate::app_pages`] `gate.rs`
//! precedent: the decorator decision kernel lives in the read-only F-06
//! foundation
//! (`pidash_auth::permissions::allow::{decide_allow, ...}` —
//! `app/permissions/base.py:19-84`, `CreatorGate::App` tree); this module
//! pins which [`Gate`] each route carries ([`GATES`]), adds the async
//! [`tenant_context`]/[`resolve_gate`] row fetching (handlers call it —
//! no handler takes an unscoped database handle for these routes), and
//! ports the flag guard with byte-exact bodies.
//!
//! Gate matrix (decorator lines; `A/M/G` = ADMIN/MEMBER/GUEST):
//!
//! | route | method | gate | flag |
//! | --- | --- | --- | --- |
//! | `schedulers/` | GET | [`Gate::WorkspaceOpen`] (`views.py:51-54`) | yes |
//! | `schedulers/` | POST | [`Gate::WorkspaceAdmin`] (`views.py:74`) | yes |
//! | `schedulers/<id>/` | GET/PATCH/DELETE | [`Gate::WorkspaceAdmin`] (`views.py:94,113,135`) | yes |
//! | `scheduler-bindings/` | GET | [`Gate::ProjectOpen`] (`views.py:168-171`) | yes |
//! | `scheduler-bindings/` | POST | [`Gate::ProjectAdmin`] (`views.py:188`) | yes |
//! | `scheduler-bindings/<id>/` | GET | [`Gate::ProjectOpen`] (`views.py:233-236`) | yes |
//! | `scheduler-bindings/<id>/` | PATCH/DELETE | [`Gate::ProjectAdmin`] (`views.py:251,280`) | yes |
//! | `scheduler-bindings/occurrences/` | GET | [`Gate::ProjectOpen`] (`occurrences.py:71-74`) | NO (quirk) |
//!
//! Gate order (preserved, not redesigned — F36-09 `check_order`):
//! DRF session authentication ([`crate::license::resolve_actor`], the
//! `BaseSessionAuthentication` + `IsAuthenticated` half of
//! `app/views/base.py:189-194`) runs before the decorator, and the
//! decorator runs before the handler body. Anonymous callers 401 before
//! any gate; denied members 403 through the decorator's fallthrough
//! body (`base.py:81-84`; the decorator defines no per-route message);
//! the flag guard 404s after the gate on the 4 CRUD routes only.
//!
//! Response shapes (handlers render [`Denial`]; recorded here so the
//! matrix has one home):
//!
//! * Anonymous on any scheduler route: 401 [`UNAUTHENTICATED_BODY`] —
//!   DRF 3.15.2 `exception_handler`'s lowercase-`detail` rendering of
//!   `NotAuthenticated` (pinned by every contract suite's `ANON`).
//! * Denied member: 403
//!   [`crate::permissions::PERMISSION_DENIED_BODY`] (reused by reference).
//! * Disabled instance on the 4 CRUD routes: 404 [`DISABLED_BODY`].
//! * Unknown workspace slug: 403, never 404 — the gate runs before any
//!   object lookup
//!   (`test_unknown_workspace_slug_denied`).
//!
//! Boundaries owned elsewhere, not here:
//!
//! * The `project_id` slug-or-UUID rewrite (`_rewrite_project_kwarg`,
//!   `app/views/base.py:48-81` + `Project.resolve`) runs in
//!   `BaseAPIView.initial`, before the gate, and is a per-handler-file
//!   `resolve_project_id` helper (the `app_analytics`/`app_assets`/
//!   `app_cycles` precedent). [`resolve_gate`] takes the resolved
//!   [`uuid::Uuid`]; the rewrite's 404 `{"detail":"Project not found"}`
//!   belongs to the handlers issues (PIDASHCONV-634/635).
//! * Object-lookup 404s (`get_object_or_404` in the handler bodies) run
//!   after the gate; the gate itself never 404s (except the flag guard).
//! * No custom throttle classes exist on these routes — nothing to port.
//!
//! Ported quirks (translate, don't redesign):
//!
//! * The occurrences endpoint never calls `_feature_enabled`
//!   (`occurrences.py:63-78`): with `SCHEDULER_ENABLED=false` the 4 CRUD
//!   routes 404 while occurrences keeps serving 200.
//! * The PROJECT-level workspace-admin bypass still requires an active
//!   project-membership row (kernel branch, `base.py:63-78`); a
//!   workspace admin without one 403s on PROJECT routes.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono_tz::Tz;

use pidash_auth::permissions::allow::{
    decide_allow, AllowFacts, AllowLevel, AllowSpec, CreatorGate,
};
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_auth::scope::TenantScope;
use pidash_db::config::Settings;
use pidash_types::WorkspaceId;

use crate::state::AppState;

/// Exact bytes of the DRF `IsAuthenticated` denial (401): what anonymous
/// callers on every scheduler route receive before any gate runs.
/// Lowercase `detail` is DRF 3.15.2 `exception_handler`'s rendering of a
/// raised `NotAuthenticated` (pinned by every contract suite's `ANON`
/// and F36-09 `bodies.401_anonymous`).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// Exact bytes of `_disabled_response` (404, `views.py:36-40`): what the
/// 4 CRUD routes answer when `SCHEDULER_ENABLED` is false. The
/// occurrences route has no flag check (ported quirk) and never renders
/// this body.
pub const DISABLED_BODY: &str = r#"{"error":"Project scheduler is disabled on this instance"}"#;
/// `BaseAPIView.handle_exception`'s generic 500 branch
/// (`app/views/base.py:211-250`): database failures and unreachable
/// caller bugs render here.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

// ---------------------------------------------------------------------------
// Gate table: which decorator each route+method carries
// ---------------------------------------------------------------------------

/// How one scheduler route+method authorizes, before any handler logic.
/// The four shapes cover the 11 (route, method) rows in [`GATES`]: two
/// `@allow_permission` levels × the open-read vs admin-write role sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// `@allow_permission([ADMIN, MEMBER, GUEST], level="WORKSPACE")`
    /// (`app/permissions/base.py:44-51`): scheduler list GET.
    WorkspaceOpen,
    /// `@allow_permission([ADMIN], level="WORKSPACE")`: scheduler
    /// create + detail GET/PATCH/DELETE.
    WorkspaceAdmin,
    /// `@allow_permission([ADMIN, MEMBER, GUEST], level="PROJECT")`
    /// (`app/permissions/base.py:52-78`): binding list/detail GET +
    /// occurrences GET.
    ProjectOpen,
    /// `@allow_permission([ADMIN], level="PROJECT")`: binding install +
    /// detail PATCH/DELETE.
    ProjectAdmin,
}

impl Gate {
    /// The decorator `level` for this gate (kernel branch).
    pub fn level(&self) -> AllowLevel {
        match self {
            Gate::WorkspaceOpen | Gate::WorkspaceAdmin => AllowLevel::Workspace,
            Gate::ProjectOpen | Gate::ProjectAdmin => AllowLevel::Project,
        }
    }

    /// The decorator `allowed_roles` for this gate, as stored `role`
    /// values (`ROLE`: ADMIN=20, MEMBER=15, GUEST=5,
    /// `app/permissions/base.py:13-17`).
    pub fn roles(&self) -> &'static [i32] {
        match self {
            Gate::WorkspaceOpen | Gate::ProjectOpen => &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST],
            Gate::WorkspaceAdmin | Gate::ProjectAdmin => &[ROLE_ADMIN],
        }
    }
}

/// One row of the gate table: a route+method and its gate.
pub struct RouteGate {
    pub method: &'static str,
    pub path: &'static str,
    pub gate: Gate,
    /// Whether this row checks `SCHEDULER_ENABLED` after the gate.
    /// False only for occurrences (`occurrences.py:63-78` never calls
    /// `_feature_enabled` — ported as-is).
    pub flag_check: bool,
    /// Python source of the gate for this row.
    pub source: &'static str,
}

/// All 5 scheduler routes, one entry per method, in URL order
/// (`apps/api/pi_dash/app/urls/scheduler.py:16-46`).
pub static GATES: &[RouteGate] = &[
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/schedulers/",
        gate: Gate::WorkspaceOpen,
        flag_check: true,
        source: "views.py:51-55 (list)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/schedulers/",
        gate: Gate::WorkspaceAdmin,
        flag_check: true,
        source: "views.py:74-75 (create)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/schedulers/<uuid:scheduler_id>/",
        gate: Gate::WorkspaceAdmin,
        flag_check: true,
        source: "views.py:94-95 (detail)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/schedulers/<uuid:scheduler_id>/",
        gate: Gate::WorkspaceAdmin,
        flag_check: true,
        source: "views.py:113-114 (detail)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/schedulers/<uuid:scheduler_id>/",
        gate: Gate::WorkspaceAdmin,
        flag_check: true,
        source: "views.py:135-136 (detail)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/scheduler-bindings/",
        gate: Gate::ProjectOpen,
        flag_check: true,
        source: "views.py:168-172 (list)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/scheduler-bindings/",
        gate: Gate::ProjectAdmin,
        flag_check: true,
        source: "views.py:188-189 (install)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/scheduler-bindings/<uuid:binding_id>/",
        gate: Gate::ProjectOpen,
        flag_check: true,
        source: "views.py:233-237 (detail)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<project_id>/scheduler-bindings/<uuid:binding_id>/",
        gate: Gate::ProjectAdmin,
        flag_check: true,
        source: "views.py:251-252 (detail)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<project_id>/scheduler-bindings/<uuid:binding_id>/",
        gate: Gate::ProjectAdmin,
        flag_check: true,
        source: "views.py:280-281 (detail)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/scheduler-bindings/occurrences/",
        gate: Gate::ProjectOpen,
        flag_check: false,
        source: "occurrences.py:71-75 (no _feature_enabled call — quirk)",
    },
];

/// Look up the gate for one route+method; `None` is not a scheduler route.
pub fn gate_for(method: &str, path: &str) -> Option<&'static RouteGate> {
    GATES
        .iter()
        .find(|row| row.method == method && row.path == path)
}

// ---------------------------------------------------------------------------
// Pure decision (kernel-backed; the F36-09 matrix replays through this)
// ---------------------------------------------------------------------------

/// Outcome of a gate check, before denial rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    /// The handler body runs (flag guard still to come on CRUD rows).
    Allow,
    /// The handler body does not run: answer the allow-style 403
    /// ([`crate::permissions::PERMISSION_DENIED_BODY`]).
    Deny,
    /// Anonymous: never reaches a gate; DRF `IsAuthenticated` denies
    /// first with [`UNAUTHENTICATED_BODY`] (401).
    Unauthenticated,
}

/// Decide one gate from pre-fetched membership facts.
///
/// `facts` mirrors one `(user, slug[, project_id])` row set: the
/// active-row filters (`is_active=True`), the soft-delete exclusion,
/// and the `workspace__slug=` scoping are the caller's SQL (the
/// [`tenant_context`] half); the scope check denies facts fetched for a
/// different workspace. Scheduler routes never set `creator`/`model`
/// (F36-09 `creator_kwarg`), so the creator bypass is always off and
/// the kernel never reads `is_workspace_member`/`is_creator` for our
/// specs — [`tenant_context`] leaves them false.
pub fn decide_gate(gate: &Gate, scope: &TenantScope, facts: &AllowFacts) -> GateOutcome {
    if !facts.authenticated {
        return GateOutcome::Unauthenticated;
    }
    // Scheduler views import the decorator from `pi_dash.app.permissions`
    // (`views.py:23`, `occurrences.py:43`): the App tree.
    let spec = AllowSpec {
        level: gate.level(),
        creator_gate: CreatorGate::App,
        creator_bypass: false,
    };
    if decide_allow(&spec, scope, facts) {
        GateOutcome::Allow
    } else {
        GateOutcome::Deny
    }
}

/// Tenant context for one request: the workspace the URL names.
/// Handlers build membership facts only for this slug, so
/// [`decide_gate`] denies cross-workspace facts even when the rows exist.
pub fn tenant_scope(slug: &str) -> TenantScope {
    TenantScope::new(WorkspaceId::from(slug))
}

// ---------------------------------------------------------------------------
// Async half: session auth + row fetching (handlers call into this)
// ---------------------------------------------------------------------------

/// The authenticated, authorized request context: who acts, in which
/// tenant, and in which timezone datetimes render. Handlers take this
/// from [`resolve_gate`] and never fetch membership rows themselves.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedGate {
    pub user_id: uuid::Uuid,
    /// `request.user.user_timezone` (`TimezoneMixin.initial` activates
    /// it before the gate in Django): both scheduler shapes render
    /// `created_at`/`updated_at` in this zone.
    pub timezone: Tz,
    pub workspace_id: uuid::Uuid,
    /// `Some` on the three project-level routes (post-rewrite id the
    /// caller passed in), `None` on the two workspace-level routes.
    pub project_id: Option<uuid::Uuid>,
}

/// What a gate check denies with, before body rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denial {
    /// No session / bad session / inactive or deleted user: 401
    /// [`UNAUTHENTICATED_BODY`].
    Unauthorized,
    /// Decorator denial: 403
    /// [`crate::permissions::PERMISSION_DENIED_BODY`].
    Forbidden,
    /// `SCHEDULER_ENABLED` is false on a CRUD route: 404 [`DISABLED_BODY`].
    FeatureDisabled,
    /// Database failure, missing pool, or an unreachable caller bug
    /// (Project gate without a project id): 500 [`SERVER_ERROR_BODY`].
    ServerError,
}

fn json_response(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("static scheduler-gate response")
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        match self {
            Denial::Unauthorized => json_response(StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY),
            Denial::Forbidden => json_response(
                StatusCode::FORBIDDEN,
                crate::permissions::PERMISSION_DENIED_BODY,
            ),
            Denial::FeatureDisabled => json_response(StatusCode::NOT_FOUND, DISABLED_BODY),
            Denial::ServerError => {
                json_response(StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_BODY)
            }
        }
    }
}

/// Session auth + the decorator's membership checks, in Django's order
/// (`app/permissions/base.py:19-84`): full session authentication first
/// (anonymous is rejected 401 before anything else), then the gate's
/// membership fetches, then the kernel decision. An unknown slug denies
/// 403 (the gate runs before any object lookup), never 404.
///
/// `project_id` is the post-rewrite id the caller resolved (the
/// `_rewrite_project_kwarg` half lives in each handler file, per the
/// `app_cycles` precedent): `Some` on the three project-level routes,
/// `None` on the two workspace-level routes (ignored there). A Project
/// gate with `None` is a caller bug with no Django analog (the URL
/// always supplies `project_id` on PROJECT routes), so it fails as
/// [`Denial::ServerError`].
///
/// The flag guard is NOT part of this function: CRUD handlers call
/// [`ensure_feature_enabled`] after it; the occurrences handler must
/// not (ported quirk).
pub async fn resolve_gate(
    state: &AppState,
    gate: &Gate,
    slug: &str,
    project_id: Option<&uuid::Uuid>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<ResolvedGate, Denial> {
    let pool = state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)?;
    let actor =
        crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
            .await
            .map_err(|_| Denial::ServerError)?
            .ok_or(Denial::Unauthorized)?;
    let tenant = tenant_context(pool, gate, slug, project_id, &actor.id).await?;
    let scope = tenant_scope(slug);
    match decide_gate(gate, &scope, &tenant.facts) {
        GateOutcome::Allow => Ok(ResolvedGate {
            user_id: actor.id,
            timezone: actor.timezone,
            workspace_id: tenant.workspace_id,
            project_id: project_id.copied(),
        }),
        GateOutcome::Deny => Err(Denial::Forbidden),
        // `tenant_context` builds facts with `authenticated: true`
        // (anonymous already returned above): unreachable.
        GateOutcome::Unauthenticated => Err(Denial::ServerError),
    }
}

/// Facts the tenant fetch returns: the kernel inputs plus the request
/// workspace id for handler scoping.
pub struct TenantFacts {
    pub facts: AllowFacts,
    pub workspace_id: uuid::Uuid,
}

/// The decorator's `Model.objects.filter(...).exists()` checks as SQL,
/// one `SELECT 1 ... LIMIT 1` per check: an active membership row with
/// a listed role for WORKSPACE gates (`base.py:44-51`); the allowed-role
/// project row, else the any-role project row plus the workspace-ADMIN
/// row, for PROJECT gates (`base.py:52-78`, short-circuiting like the
/// `if`/`elif`). Soft-deleted rows never count (the default manager);
/// inactive rows never count (`is_active=True`).
///
/// An unknown slug denies here ([`Denial::Forbidden`]): no membership
/// row can join it, exactly the decorator's fallthrough.
pub async fn tenant_context(
    pool: &sqlx::PgPool,
    gate: &Gate,
    slug: &str,
    project_id: Option<&uuid::Uuid>,
    user_id: &uuid::Uuid,
) -> Result<TenantFacts, Denial> {
    let workspace_id = workspace_id(pool, slug).await?.ok_or(Denial::Forbidden)?;
    let admin_only = matches!(gate, Gate::WorkspaceAdmin | Gate::ProjectAdmin);
    let facts = match gate.level() {
        AllowLevel::Workspace => AllowFacts {
            workspace: WorkspaceId::from(slug),
            authenticated: true,
            is_workspace_member: false,
            has_allowed_workspace_role: exists_workspace_role(pool, slug, user_id, admin_only)
                .await?,
            is_creator: false,
            has_allowed_project_role: false,
            is_project_member: false,
            is_workspace_admin: false,
        },
        AllowLevel::Project => {
            let project_id = project_id.copied().ok_or(Denial::ServerError)?;
            let has_allowed =
                exists_project_role(pool, slug, &project_id, user_id, admin_only).await?;
            // The decorator's `elif`: an allowed-role row IS a project
            // row, so the bypass pair is only fetched when it misses.
            let (is_member, is_admin) = if has_allowed {
                (true, false)
            } else {
                let is_member =
                    exists_any_project_membership(pool, slug, &project_id, user_id).await?;
                let is_admin = if is_member {
                    exists_workspace_role(pool, slug, user_id, true).await?
                } else {
                    // No project row: the bypass conjunct is already
                    // false; skip the admin query like the `and`.
                    false
                };
                (is_member, is_admin)
            };
            AllowFacts {
                workspace: WorkspaceId::from(slug),
                authenticated: true,
                is_workspace_member: false,
                has_allowed_workspace_role: false,
                is_creator: false,
                has_allowed_project_role: has_allowed,
                is_project_member: is_member,
                is_workspace_admin: is_admin,
            }
        }
    };
    Ok(TenantFacts {
        facts,
        workspace_id,
    })
}

/// `workspaces` id for the URL slug. `None` is an unknown slug, which
/// denies at the gate (403), never 404s.
async fn workspace_id(pool: &sqlx::PgPool, slug: &str) -> Result<Option<uuid::Uuid>, Denial> {
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as(r#"SELECT w.id FROM workspaces w WHERE w.slug = $1"#)
            .bind(slug)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.0))
}

/// `WorkspaceMember.objects.filter(member, workspace__slug=slug,
/// role__in, is_active=True).exists()` (`base.py:44-51`, and the
/// `role=ADMIN` bypass row at `:69-76` when `admin_only`). The role
/// literals are the `ROLE` values (`base.py:13-17`); the `#[cfg(test)]`
/// module pins them against the foundation constants.
async fn exists_workspace_role(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
    admin_only: bool,
) -> Result<bool, Denial> {
    // `SELECT 1` yields int4: decode as `i32`.
    let row: Option<(i32,)> = if admin_only {
        sqlx::query_as(
            r#"SELECT 1 FROM workspace_members wm
               JOIN workspaces w ON w.id = wm.workspace_id
               WHERE wm.member_id = $1 AND w.slug = $2 AND wm.role = 20
               AND wm.is_active AND wm.deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    } else {
        sqlx::query_as(
            r#"SELECT 1 FROM workspace_members wm
               JOIN workspaces w ON w.id = wm.workspace_id
               WHERE wm.member_id = $1 AND w.slug = $2 AND wm.role IN (20, 15, 5)
               AND wm.is_active AND wm.deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    };
    Ok(row.is_some())
}

/// `ProjectMember.objects.filter(member, workspace__slug=slug,
/// project_id, role__in, is_active=True).exists()` (`base.py:52-60`).
async fn exists_project_role(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    admin_only: bool,
) -> Result<bool, Denial> {
    let row: Option<(i32,)> = if admin_only {
        sqlx::query_as(
            r#"SELECT 1 FROM project_members pm
               JOIN workspaces w ON w.id = pm.workspace_id
               WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3 AND pm.role = 20
               AND pm.is_active AND pm.deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    } else {
        sqlx::query_as(
            r#"SELECT 1 FROM project_members pm
               JOIN workspaces w ON w.id = pm.workspace_id
               WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
               AND pm.role IN (20, 15, 5)
               AND pm.is_active AND pm.deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    };
    Ok(row.is_some())
}

/// The bypass's project-membership conjunct (`base.py:64-69`): any
/// active project row, regardless of role.
async fn exists_any_project_membership(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    let row: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL
           LIMIT 1"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

// ---------------------------------------------------------------------------
// Feature-flag guard (`_feature_enabled`, views.py:32-40)
// ---------------------------------------------------------------------------

/// `_feature_enabled` (`views.py:32-33`):
/// `getattr(settings, "SCHEDULER_ENABLED", True)`, where the setting
/// resolves as `get_config("SCHEDULER_ENABLED", "true").lower() ==
/// "true"` (`settings/common.py:446`) — missing means enabled. The
/// registry half is `Settings::scheduler_enabled`
/// (`crates/db/src/config/settings.rs`, `flag_true` with the same
/// default); this predicate reads it off the request state so handlers
/// never touch the environment per request.
pub fn feature_enabled(settings: &Settings) -> bool {
    settings.scheduler_enabled
}

/// Run the flag guard after [`resolve_gate`] on the 4 CRUD routes: a
/// disabled instance answers 404 [`DISABLED_BODY`] instead of the
/// handler body (`views.py:56,76,96,115,137,173,190,238,253,282`). The
/// occurrences handler must NOT call this (ported quirk: the Python
/// endpoint never checks the flag).
pub fn ensure_feature_enabled(settings: &Settings) -> Result<(), Denial> {
    if feature_enabled(settings) {
        Ok(())
    } else {
        Err(Denial::FeatureDisabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> TenantScope {
        tenant_scope("acme")
    }

    fn anon_facts() -> AllowFacts {
        AllowFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: false,
            is_workspace_member: false,
            has_allowed_workspace_role: false,
            is_creator: false,
            has_allowed_project_role: false,
            is_project_member: false,
            is_workspace_admin: false,
        }
    }

    /// Facts for an authenticated caller whose workspace role is `role`
    /// (active row in the request workspace; `None` = the contract
    /// suite's outsider), against `gate`'s role list — what
    /// [`tenant_context`] builds on WORKSPACE rows.
    fn ws_facts(role: Option<i32>, gate: &Gate) -> AllowFacts {
        let allowed = gate.roles();
        AllowFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            is_workspace_member: false,
            has_allowed_workspace_role: role.map(|r| allowed.contains(&r)).unwrap_or(false),
            is_creator: false,
            has_allowed_project_role: false,
            is_project_member: false,
            is_workspace_admin: false,
        }
    }

    /// Facts for an authenticated caller whose project role is `prole`
    /// (`None` = no project row) and who is a workspace admin iff
    /// `ws_admin` — what [`tenant_context`] builds on PROJECT rows
    /// (an allowed-role row short-circuits the bypass pair, like the
    /// decorator's `if`/`elif`).
    fn proj_facts(prole: Option<i32>, ws_admin: bool, gate: &Gate) -> AllowFacts {
        let has_allowed = prole.map(|r| gate.roles().contains(&r)).unwrap_or(false);
        let (is_member, is_admin) = if has_allowed {
            (true, false)
        } else {
            (prole.is_some(), prole.is_some() && ws_admin)
        };
        AllowFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            is_workspace_member: false,
            has_allowed_workspace_role: false,
            is_creator: false,
            has_allowed_project_role: has_allowed,
            is_project_member: is_member,
            is_workspace_admin: is_admin,
        }
    }

    fn decide(gate: &Gate, facts: &AllowFacts) -> GateOutcome {
        decide_gate(gate, &scope(), facts)
    }

    /// F36-09 `gate_semantics.roles` (`app/permissions/base.py:13-17`).
    #[test]
    fn role_values_match_python() {
        assert_eq!(ROLE_ADMIN, 20);
        assert_eq!(ROLE_MEMBER, 15);
        assert_eq!(ROLE_GUEST, 5);
        assert_eq!(Gate::WorkspaceOpen.roles(), &[20, 15, 5]);
        assert_eq!(Gate::ProjectOpen.roles(), &[20, 15, 5]);
        assert_eq!(Gate::WorkspaceAdmin.roles(), &[20]);
        assert_eq!(Gate::ProjectAdmin.roles(), &[20]);
        assert_eq!(Gate::WorkspaceOpen.level(), AllowLevel::Workspace);
        assert_eq!(Gate::WorkspaceAdmin.level(), AllowLevel::Workspace);
        assert_eq!(Gate::ProjectOpen.level(), AllowLevel::Project);
        assert_eq!(Gate::ProjectAdmin.level(), AllowLevel::Project);
    }

    /// F36-09 `routes`: 11 (route, method) rows over the 5 URL routes,
    /// each carrying the decorator its Python method carries.
    #[test]
    fn gate_table_pins_every_row() {
        assert_eq!(GATES.len(), 11);
        let sched_list = "workspaces/<slug>/schedulers/";
        let sched_detail = "workspaces/<slug>/schedulers/<uuid:scheduler_id>/";
        let bind_list = "workspaces/<slug>/projects/<project_id>/scheduler-bindings/";
        let bind_detail =
            "workspaces/<slug>/projects/<project_id>/scheduler-bindings/<uuid:binding_id>/";
        let occ = "workspaces/<slug>/projects/<project_id>/scheduler-bindings/occurrences/";
        let cases = [
            ("GET", sched_list, Gate::WorkspaceOpen, true),
            ("POST", sched_list, Gate::WorkspaceAdmin, true),
            ("GET", sched_detail, Gate::WorkspaceAdmin, true),
            ("PATCH", sched_detail, Gate::WorkspaceAdmin, true),
            ("DELETE", sched_detail, Gate::WorkspaceAdmin, true),
            ("GET", bind_list, Gate::ProjectOpen, true),
            ("POST", bind_list, Gate::ProjectAdmin, true),
            ("GET", bind_detail, Gate::ProjectOpen, true),
            ("PATCH", bind_detail, Gate::ProjectAdmin, true),
            ("DELETE", bind_detail, Gate::ProjectAdmin, true),
            ("GET", occ, Gate::ProjectOpen, false),
        ];
        for (method, path, gate, flag) in cases {
            let row = gate_for(method, path).expect("every matrix row resolves");
            assert_eq!(row.gate, gate, "{method} {path}");
            assert_eq!(row.flag_check, flag, "{method} {path}");
            assert!(!row.source.is_empty());
        }
        assert!(gate_for("PUT", sched_list).is_none());
        assert!(gate_for("GET", "workspaces/<slug>/nope/").is_none());
    }

    /// F36-09 `flag.routes_with_flag_check` + `occurrences_has_no_flag_check`.
    #[test]
    fn flag_check_covers_crud_only() {
        for row in GATES {
            if row.path.ends_with("/occurrences/") {
                assert!(!row.flag_check, "occurrences never checks the flag");
            } else {
                assert!(
                    row.flag_check,
                    "{} {} checks the flag",
                    row.method, row.path
                );
            }
        }
    }

    /// F36-09 `tenant_cases`: anonymous 401s before any gate on all 11 rows.
    #[test]
    fn anonymous_denies_before_any_gate() {
        for row in GATES {
            assert_eq!(
                decide(&row.gate, &anon_facts()),
                GateOutcome::Unauthenticated,
                "{} {}",
                row.method,
                row.path
            );
        }
    }

    /// F36-09 matrix, WORKSPACE rows: list GET open to A/M/G
    /// (`test_reads_open_to_all_project_roles`); create + detail
    /// admin-only (`test_create_scheduler_forbidden_for_member`,
    /// `test_scheduler_detail_admin_only`); outsiders denied
    /// (`test_outsider_denied_everywhere`).
    #[test]
    fn workspace_gate_matrix() {
        // Open: every role passes, outsiders fail.
        for role in [ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST] {
            assert_eq!(
                decide(
                    &Gate::WorkspaceOpen,
                    &ws_facts(Some(role), &Gate::WorkspaceOpen)
                ),
                GateOutcome::Allow,
                "open role {role}"
            );
        }
        assert_eq!(
            decide(&Gate::WorkspaceOpen, &ws_facts(None, &Gate::WorkspaceOpen)),
            GateOutcome::Deny
        );
        // Admin: members and guests fail, outsiders fail.
        assert_eq!(
            decide(
                &Gate::WorkspaceAdmin,
                &ws_facts(Some(ROLE_ADMIN), &Gate::WorkspaceAdmin)
            ),
            GateOutcome::Allow
        );
        for role in [ROLE_MEMBER, ROLE_GUEST] {
            assert_eq!(
                decide(
                    &Gate::WorkspaceAdmin,
                    &ws_facts(Some(role), &Gate::WorkspaceAdmin)
                ),
                GateOutcome::Deny,
                "admin-only role {role}"
            );
        }
        assert_eq!(
            decide(
                &Gate::WorkspaceAdmin,
                &ws_facts(None, &Gate::WorkspaceAdmin)
            ),
            GateOutcome::Deny
        );
    }

    /// F36-09 matrix, PROJECT rows: binding list/detail GET + occurrences
    /// open to A/M/G (`test_reads_open_to_all_project_roles`); install +
    /// detail PATCH/DELETE admin-only (`test_binding_writes_admin_only`,
    /// `test_member_of_other_project_denied`).
    #[test]
    fn project_gate_matrix() {
        for role in [ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST] {
            assert_eq!(
                decide(
                    &Gate::ProjectOpen,
                    &proj_facts(Some(role), false, &Gate::ProjectOpen)
                ),
                GateOutcome::Allow,
                "open role {role}"
            );
        }
        assert_eq!(
            decide(
                &Gate::ProjectOpen,
                &proj_facts(None, false, &Gate::ProjectOpen)
            ),
            GateOutcome::Deny
        );
        assert_eq!(
            decide(
                &Gate::ProjectAdmin,
                &proj_facts(Some(ROLE_ADMIN), false, &Gate::ProjectAdmin)
            ),
            GateOutcome::Allow
        );
        for role in [ROLE_MEMBER, ROLE_GUEST] {
            assert_eq!(
                decide(
                    &Gate::ProjectAdmin,
                    &proj_facts(Some(role), false, &Gate::ProjectAdmin)
                ),
                GateOutcome::Deny,
                "admin-only role {role}"
            );
        }
        assert_eq!(
            decide(
                &Gate::ProjectAdmin,
                &proj_facts(None, false, &Gate::ProjectAdmin)
            ),
            GateOutcome::Deny
        );
    }

    /// F36-09 `gate_semantics.PROJECT`: the workspace-admin bypass needs
    /// an active project row (`base.py:63-78`).
    /// `test_workspace_admin_without_project_membership_cannot_install`
    /// pins the deny half; the allow half is the decorator's `elif`.
    #[test]
    fn project_bypass_needs_project_membership() {
        // Project member (any role) + workspace admin: allowed.
        assert_eq!(
            decide(
                &Gate::ProjectAdmin,
                &proj_facts(Some(ROLE_MEMBER), true, &Gate::ProjectAdmin)
            ),
            GateOutcome::Allow
        );
        // Workspace admin with NO project row: denied.
        assert_eq!(
            decide(
                &Gate::ProjectAdmin,
                &proj_facts(None, true, &Gate::ProjectAdmin)
            ),
            GateOutcome::Deny
        );
        // Project member without workspace admin: denied on admin gates.
        assert_eq!(
            decide(
                &Gate::ProjectAdmin,
                &proj_facts(Some(ROLE_GUEST), false, &Gate::ProjectAdmin)
            ),
            GateOutcome::Deny
        );
    }

    /// F36-09 `tenant_cases`: facts fetched for another workspace deny
    /// even with an allowed role (`test_admin_of_other_workspace_denied`;
    /// the kernel's scope check).
    #[test]
    fn cross_workspace_facts_deny() {
        let other = TenantScope::new(WorkspaceId::from("other"));
        let facts = ws_facts(Some(ROLE_ADMIN), &Gate::WorkspaceOpen);
        assert_eq!(
            decide_gate(&Gate::WorkspaceOpen, &other, &facts),
            GateOutcome::Deny
        );
        let facts = proj_facts(Some(ROLE_ADMIN), false, &Gate::ProjectAdmin);
        assert_eq!(
            decide_gate(&Gate::ProjectAdmin, &other, &facts),
            GateOutcome::Deny
        );
    }

    /// F36-09 `flag`: `SCHEDULER_ENABLED` defaults true
    /// (`settings/common.py:446`); disabled answers 404 on CRUD rows.
    #[test]
    fn flag_guard_defaults_enabled() {
        let settings = Settings::test_defaults();
        assert!(feature_enabled(&settings));
        assert!(ensure_feature_enabled(&settings).is_ok());
        let mut disabled = Settings::test_defaults();
        disabled.scheduler_enabled = false;
        assert!(!feature_enabled(&disabled));
        assert_eq!(
            ensure_feature_enabled(&disabled),
            Err(Denial::FeatureDisabled)
        );
    }

    /// Every denial body byte, as the contract suites pin them.
    #[test]
    fn denial_bodies_match_contract_goldens() {
        assert_eq!(
            UNAUTHENTICATED_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            crate::permissions::PERMISSION_DENIED_BODY,
            r#"{"error":"You don't have the required permissions."}"#
        );
        assert_eq!(
            DISABLED_BODY,
            r#"{"error":"Project scheduler is disabled on this instance"}"#
        );
        assert_eq!(
            SERVER_ERROR_BODY,
            r#"{"error":"Something went wrong please try again later"}"#
        );
    }
}
