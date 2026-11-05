//! Non-HTTP service entries for the v1 scheduler routes (NEWPIDASH-37).
//!
//! One entry per route the MCP scheduler tools mirror
//! (`docs/mcp-connector.md`, "Schedulers"): the three reads plus the
//! seven PDASHOSS01-278/318 writes. Each entry runs the route's gates,
//! the `SCHEDULER_ENABLED` flag and the shared handler core, and renders
//! the exact status + body bytes the v1 route sends — so the overlay
//! calls OSS instead of re-porting the serializers.
//!
//! Split of duties with the v1 handlers: the handlers keep HTTP transport
//! (API-key auth, the project rewrite, body parsing, `If-Match` joining,
//! Django proxy branches) and call the same cores these entries call, so
//! the two surfaces cannot drift. The caller authenticates (the MCP
//! principal is already authed) and resolves nothing: entries take the
//! raw workspace slug and project ref and run the v1 rewrite and gates
//! themselves, in Django's order.

use std::collections::HashMap;

use axum::response::Response;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use pidash_compat::http::{OneOrMany, QueryMap};

use super::handlers;
use super::perms;
use super::writes;
use super::{
    activate_timezone, ensure_feature_enabled, require_project_entity, require_workspace_entity,
    rewrite_project_id,
};

/// Re-exported so non-HTTP callers can name the entries' error type.
pub use super::Denial;

/// Drain cap for a rendered body: list pages carry up to 1000 full
/// definitions with prompts, so this must clear the largest page.
const BODY_LIMIT: usize = 64 * 1024 * 1024;

/// The resolved caller of one service call: pool, user, workspace slug
/// and the raw stored time-zone name (`None` renders UTC, as MCP does —
/// it passes no request zone).
pub struct ServiceCtx {
    pub pool: PgPool,
    pub user_id: Uuid,
    pub workspace_slug: String,
    pub actor_timezone: Option<String>,
}

/// The list query params (`?per_page=&cursor=&fields=&expand=`), as the
/// v1 routes receive them. Single-valued: the routes collapse repeats to
/// the last value, and MCP never sends repeats.
pub struct ListParams {
    pub per_page: Option<String>,
    pub cursor: Option<String>,
    pub fields: Option<String>,
    pub expand: Option<String>,
}

impl ListParams {
    /// No params: every default the v1 route applies.
    pub fn empty() -> Self {
        Self {
            per_page: None,
            cursor: None,
            fields: None,
            expand: None,
        }
    }

    fn query_map(&self) -> QueryMap {
        let mut map: QueryMap = HashMap::with_capacity(4);
        for (key, value) in [
            ("per_page", &self.per_page),
            ("cursor", &self.cursor),
            ("fields", &self.fields),
            ("expand", &self.expand),
        ] {
            if let Some(text) = value {
                map.insert(key.to_owned(), OneOrMany::One(text.clone()));
            }
        }
        map
    }
}

/// A rendered outcome: the status plus the exact body bytes the v1 route
/// sends (empty for the 204s).
pub struct Outcome {
    pub status: u16,
    pub body: String,
}

async fn drain(response: Response) -> (u16, String) {
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), BODY_LIMIT)
        .await
        .unwrap_or_default();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// Render a denial to the status + body the v1 route sends. Every arm but
/// [`Denial::App`] renders from the shared table; `App` bodies (the web
/// app write path's already-rendered failures) drain byte-identically.
pub async fn denial_parts(denial: Denial) -> (u16, String) {
    match denial {
        Denial::App(response) => drain(*response).await,
        other => {
            let (status, body) = other.status_and_body();
            (status.as_u16(), body)
        }
    }
}

async fn outcome_of(result: Result<Response, Denial>) -> Result<Outcome, Denial> {
    match result {
        Ok(response) => {
            let (status, body) = drain(response).await;
            Ok(Outcome { status, body })
        }
        Err(denial) => Err(denial),
    }
}

/// The workspace id for a slug: `None` when unknown. The same lookup
/// the v1 [`super::preamble`] runs (one implementation, in
/// [`super::workspace_id_by_slug`]), so the unknown-slug 403 trap
/// behaves identically here.
async fn workspace_id(pool: &PgPool, slug: &str) -> Result<Option<Uuid>, Denial> {
    super::workspace_id_by_slug(pool, slug).await
}

/// The write gates in [`writes`] order (facts, class, zone, decorator,
/// flag), over an already-authenticated caller. The v1 handlers run the
/// same decisions through `write_context`; this is the non-HTTP twin.
async fn authorize_write(
    ctx: &ServiceCtx,
    settings: &pidash_db::config::Settings,
    project_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    scope: writes::Scope,
) -> Result<writes::WriteContext, Denial> {
    let facts = writes::role_facts(
        &ctx.pool,
        workspace_id.as_ref(),
        &ctx.user_id,
        project_id.as_ref(),
    )
    .await?;
    let (class, decorator) = match scope {
        writes::Scope::Project => (
            writes::project_class_allows(&facts),
            writes::project_admin_decorator_allows(&facts),
        ),
        writes::Scope::Workspace => (
            writes::workspace_class_allows(&facts),
            writes::workspace_admin_decorator_allows(&facts),
        ),
    };
    if !class {
        return Err(Denial::Forbidden);
    }
    let timezone = activate_timezone(ctx.actor_timezone.as_deref())?;
    if !decorator {
        return Err(Denial::RoleDenied);
    }
    ensure_feature_enabled(settings)?;
    Ok(writes::WriteContext {
        pool: ctx.pool.clone(),
        user_id: ctx.user_id,
        timezone,
        project_id,
        workspace_id,
    })
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// `GET /api/v1/workspaces/<slug>/schedulers/`: workspace definitions
/// with the caller's visible bindings, paginated.
pub async fn list_definitions(
    ctx: &ServiceCtx,
    settings: &pidash_db::config::Settings,
    params: &ListParams,
) -> Result<Outcome, Denial> {
    let wid = workspace_id(&ctx.pool, &ctx.workspace_slug).await?;
    require_workspace_entity(&ctx.pool, wid.as_ref(), &ctx.workspace_slug, &ctx.user_id).await?;
    ensure_feature_enabled(settings)?;
    let timezone = activate_timezone(ctx.actor_timezone.as_deref())?;
    let workspace_id = wid.ok_or(Denial::Forbidden)?;
    let query = params.query_map();
    outcome_of(
        handlers::ws_scheduler_list_core(
            &ctx.pool,
            &ctx.user_id,
            &workspace_id,
            &ctx.workspace_slug,
            &timezone,
            &query,
        )
        .await,
    )
    .await
}

/// `GET .../projects/<project>/schedulers/`: definitions installed on
/// this project, each with its binding here, paginated. `project_raw` is
/// the identifier or UUID, rewritten exactly as the v1 route rewrites it.
pub async fn list_project_schedulers(
    ctx: &ServiceCtx,
    settings: &pidash_db::config::Settings,
    project_raw: &str,
    params: &ListParams,
) -> Result<Outcome, Denial> {
    let wid = workspace_id(&ctx.pool, &ctx.workspace_slug).await?;
    let project_id = rewrite_project_id(&ctx.pool, &ctx.workspace_slug, project_raw).await?;
    require_project_entity(
        &ctx.pool,
        wid.as_ref(),
        &ctx.workspace_slug,
        &ctx.user_id,
        &project_id,
        perms::V1Route::ProjectList,
    )
    .await?;
    ensure_feature_enabled(settings)?;
    let timezone = activate_timezone(ctx.actor_timezone.as_deref())?;
    let workspace_id = wid.ok_or(Denial::Forbidden)?;
    let query = params.query_map();
    outcome_of(
        handlers::project_scheduler_list_core(
            &ctx.pool,
            &workspace_id,
            &ctx.workspace_slug,
            &project_id,
            &timezone,
            &query,
        )
        .await,
    )
    .await
}

/// `GET .../projects/<project>/schedulers/<id>/`: one full definition
/// with its binding here. 404 unless installed on this project.
pub async fn get_scheduler(
    ctx: &ServiceCtx,
    settings: &pidash_db::config::Settings,
    project_raw: &str,
    scheduler_id: &Uuid,
) -> Result<Outcome, Denial> {
    let wid = workspace_id(&ctx.pool, &ctx.workspace_slug).await?;
    let project_id = rewrite_project_id(&ctx.pool, &ctx.workspace_slug, project_raw).await?;
    require_project_entity(
        &ctx.pool,
        wid.as_ref(),
        &ctx.workspace_slug,
        &ctx.user_id,
        &project_id,
        perms::V1Route::ProjectDetail,
    )
    .await?;
    ensure_feature_enabled(settings)?;
    let timezone = activate_timezone(ctx.actor_timezone.as_deref())?;
    outcome_of(
        handlers::project_scheduler_detail_core(
            &ctx.pool,
            &ctx.workspace_slug,
            &project_id,
            scheduler_id,
            &timezone,
        )
        .await,
    )
    .await
}

/// `GET .../projects/<project>/schedulers/<id>/runs/`: recent runs fired
/// by this install, newest first, paginated.
pub async fn list_runs(
    ctx: &ServiceCtx,
    settings: &pidash_db::config::Settings,
    project_raw: &str,
    scheduler_id: &Uuid,
    params: &ListParams,
) -> Result<Outcome, Denial> {
    let wid = workspace_id(&ctx.pool, &ctx.workspace_slug).await?;
    let project_id = rewrite_project_id(&ctx.pool, &ctx.workspace_slug, project_raw).await?;
    require_project_entity(
        &ctx.pool,
        wid.as_ref(),
        &ctx.workspace_slug,
        &ctx.user_id,
        &project_id,
        perms::V1Route::ProjectRuns,
    )
    .await?;
    ensure_feature_enabled(settings)?;
    let timezone = activate_timezone(ctx.actor_timezone.as_deref())?;
    let query = params.query_map();
    outcome_of(
        handlers::project_scheduler_runs_core(
            &ctx.pool,
            &ctx.user_id,
            &ctx.workspace_slug,
            &project_id,
            scheduler_id,
            &timezone,
            &query,
        )
        .await,
    )
    .await
}

// ---------------------------------------------------------------------------
// Binding writes (project admin)
// ---------------------------------------------------------------------------

/// `PATCH .../projects/<project>/schedulers/<id>/binding/`: edit the
/// install. `if_match` is the joined `If-Match` header value, if sent;
/// the body's `expected_updated_at` still applies without one.
pub async fn patch_binding(
    ctx: &ServiceCtx,
    settings: &pidash_db::config::Settings,
    project_raw: &str,
    scheduler_id: &Uuid,
    data: &Value,
    if_match: Option<String>,
) -> Result<Outcome, Denial> {
    let wid = workspace_id(&ctx.pool, &ctx.workspace_slug).await?;
    let project_id = rewrite_project_id(&ctx.pool, &ctx.workspace_slug, project_raw).await?;
    let context =
        authorize_write(ctx, settings, Some(project_id), wid, writes::Scope::Project).await?;
    outcome_of(
        writes::binding_patch_inner(&context, &ctx.workspace_slug, scheduler_id, data, if_match)
            .await,
    )
    .await
}

/// `POST .../projects/<project>/schedulers/<id>/run-now/`: make the
/// install due so the scanner fires it on its next tick.
pub async fn run_now(
    ctx: &ServiceCtx,
    settings: &pidash_db::config::Settings,
    project_raw: &str,
    scheduler_id: &Uuid,
    if_match: Option<String>,
) -> Result<Outcome, Denial> {
    let wid = workspace_id(&ctx.pool, &ctx.workspace_slug).await?;
    let project_id = rewrite_project_id(&ctx.pool, &ctx.workspace_slug, project_raw).await?;
    let context =
        authorize_write(ctx, settings, Some(project_id), wid, writes::Scope::Project).await?;
    outcome_of(writes::run_now_core(&context, &ctx.workspace_slug, scheduler_id, if_match).await)
        .await
}

/// `POST .../projects/<project>/schedulers/`: install a scheduler,
/// named by id or slug in the body's `scheduler` key.
pub async fn install_binding(
    ctx: &ServiceCtx,
    settings: &pidash_db::config::Settings,
    project_raw: &str,
    data: &Value,
) -> Result<Outcome, Denial> {
    let wid = workspace_id(&ctx.pool, &ctx.workspace_slug).await?;
    let project_id = rewrite_project_id(&ctx.pool, &ctx.workspace_slug, project_raw).await?;
    let context =
        authorize_write(ctx, settings, Some(project_id), wid, writes::Scope::Project).await?;
    outcome_of(writes::binding_install_inner(&context, &ctx.workspace_slug, data).await).await
}

/// `DELETE .../projects/<project>/schedulers/<id>/binding/`: uninstall.
pub async fn uninstall_binding(
    ctx: &ServiceCtx,
    settings: &pidash_db::config::Settings,
    project_raw: &str,
    scheduler_id: &Uuid,
    if_match: Option<String>,
) -> Result<Outcome, Denial> {
    let wid = workspace_id(&ctx.pool, &ctx.workspace_slug).await?;
    let project_id = rewrite_project_id(&ctx.pool, &ctx.workspace_slug, project_raw).await?;
    let context =
        authorize_write(ctx, settings, Some(project_id), wid, writes::Scope::Project).await?;
    outcome_of(
        writes::binding_uninstall_core(&context, &ctx.workspace_slug, scheduler_id, if_match).await,
    )
    .await
}

// ---------------------------------------------------------------------------
// Definition writes (workspace admin)
// ---------------------------------------------------------------------------

/// `POST /api/v1/workspaces/<slug>/schedulers/`: create a definition.
pub async fn create_definition(
    ctx: &ServiceCtx,
    settings: &pidash_db::config::Settings,
    data: &Value,
) -> Result<Outcome, Denial> {
    let wid = workspace_id(&ctx.pool, &ctx.workspace_slug).await?;
    let context = authorize_write(ctx, settings, None, wid, writes::Scope::Workspace).await?;
    outcome_of(writes::definition_create_inner(&context, data).await).await
}

/// `PATCH /api/v1/workspaces/<slug>/schedulers/<id>/`: edit a
/// user-created definition. Built-ins answer 409 `builtin_scheduler`.
pub async fn patch_definition(
    ctx: &ServiceCtx,
    settings: &pidash_db::config::Settings,
    scheduler_id: &Uuid,
    data: &Value,
    if_match: Option<String>,
) -> Result<Outcome, Denial> {
    let wid = workspace_id(&ctx.pool, &ctx.workspace_slug).await?;
    let context = authorize_write(ctx, settings, None, wid, writes::Scope::Workspace).await?;
    outcome_of(
        writes::definition_patch_inner(&context, &ctx.workspace_slug, scheduler_id, data, if_match)
            .await,
    )
    .await
}

/// `DELETE /api/v1/workspaces/<slug>/schedulers/<id>/`: soft-delete a
/// user-created definition and every install of it.
pub async fn delete_definition(
    ctx: &ServiceCtx,
    settings: &pidash_db::config::Settings,
    scheduler_id: &Uuid,
    if_match: Option<String>,
) -> Result<Outcome, Denial> {
    let wid = workspace_id(&ctx.pool, &ctx.workspace_slug).await?;
    let context = authorize_write(ctx, settings, None, wid, writes::Scope::Workspace).await?;
    outcome_of(
        writes::definition_delete_core(&context, &ctx.workspace_slug, scheduler_id, if_match).await,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_params_empty_builds_an_empty_map() {
        let map = ListParams::empty().query_map();
        assert!(map.is_empty());
    }

    #[test]
    fn list_params_set_builds_single_values() {
        let params = ListParams {
            per_page: Some("30".to_owned()),
            cursor: None,
            fields: Some("id,slug".to_owned()),
            expand: None,
        };
        let map = params.query_map();
        assert!(matches!(
            map.get("per_page"),
            Some(OneOrMany::One(text)) if text == "30"
        ));
        assert!(matches!(
            map.get("fields"),
            Some(OneOrMany::One(text)) if text == "id,slug"
        ));
        assert!(!map.contains_key("cursor"));
        assert!(!map.contains_key("expand"));
    }

    #[tokio::test]
    async fn denial_parts_renders_the_shared_table() {
        assert_eq!(
            denial_parts(Denial::Forbidden).await,
            (403, perms::CLASS_DENIAL_BODY.to_owned(),)
        );
        assert_eq!(
            denial_parts(Denial::BadError("nope".to_owned())).await,
            (400, r#"{"error":"nope"}"#.to_owned())
        );
    }

    #[tokio::test]
    async fn denial_parts_drains_app_bodies_byte_identically() {
        use axum::response::IntoResponse;
        let rendered = Denial::BadError("inner".to_owned()).into_response();
        let (status, body) = denial_parts(Denial::App(Box::new(rendered))).await;
        assert_eq!((status, body.as_str()), (400, r#"{"error":"inner"}"#));
    }
}
