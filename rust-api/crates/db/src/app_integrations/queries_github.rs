//! GitHub-integration query reads (D-33, stage 5).
//!
//! Ports the database read shapes behind
//! `apps/api/pi_dash/app/views/integration/github.py` — the workspace
//! connect/disconnect/status/repos reads (`:426-606`), the app-status
//! ADMIN membership scoping (`:611-655`, ADMIN read at `:629`), the
//! install-session callback read (`:751-755`), and the project
//! sync/binding status reads (`:1093-1172`) — plus the pure read-helper
//! shapes in `apps/api/pi_dash/utils/github_app_auth.py`
//! (`build_app_jwt` / `app_headers` / `user_headers` /
//! `exchange_user_code` / `list_user_installations`, no live calls).
//! Fixtures FX-GHA-01, FX-GHA-02, FX-GHA-03
//! (`rust-api/fixtures/app_integrations/fx-gha-0*.json`).
//!
//! Same conventions as [`super::queries_git`]: sea-query builders,
//! runtime `sqlx::query`, executors generic over `sqlx::Executor`, row
//! structs reused from the D-05 models (`crate::integrations::*`,
//! PIDASHCONV-42, Done), joined columns aliased `<table>__<col>`.
//!
//! Boundaries (owned by sibling sub-issues, not ported here):
//!
//! * Writes (connect/disconnect `config` updates, upserts, session
//!   cleanup, delivery dedupe) belong to the handlers/tasks layers
//!   (PIDASHCONV-439/443/446/450); only the read halves live here.
//! * `verify_webhook_signature` belongs to PIDASHCONV-436 (HMAC guard).
//! * Permission outcomes belong to PIDASHCONV-436.
//!
//! SQL semantics are Django's, quirks included:
//!
//! * Default-manager `deleted_at IS NULL` on every table, including the
//!   joined `workspaces` row.
//! * `BUG (ported):` the install-callback expiry uses `expires_at <= now`
//!   (`github.py:765`) while the lazy-cleanup sweep uses
//!   `expires_at__lt=now` (`github.py:173`) — a session expiring at
//!   exactly `now` fails the callback but is not yet swept. Both
//!   operators are preserved as written.

use sea_query::{Alias, Condition, Expr, JoinType, Order, Query};
use sqlx::postgres::PgRow;
use sqlx::Row;

use super::{
    GITHUB_PROVIDER, INTEGRATION_TABLE, ROLE_ADMIN, WORKSPACE_INTEGRATION_TABLE,
    WORKSPACE_MEMBER_TABLE, WORKSPACE_TABLE,
};
use crate::integrations::git_models::{
    git_provider_account, git_repository, git_repository_binding,
};
use crate::integrations::github_models::{
    github_app_install_session::{self, GithubAppInstallSession},
    github_repository::TABLE as GH_REPO_TABLE,
    github_repository_sync::{self, GithubRepositorySync},
};

// ---------------------------------------------------------------------------
// Integration / workspace-integration reads (github.py:90-137)
// ---------------------------------------------------------------------------

/// `Integration.objects.filter(provider="github").first()`
/// (`github.py:103`, inside `_get_workspace_integration`). No binds.
pub fn integration_by_provider_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(INTEGRATION_TABLE.to_owned()));
    sel.column((Alias::new(INTEGRATION_TABLE), Alias::new("id")));
    sel.column((Alias::new(INTEGRATION_TABLE), Alias::new("provider")));
    sel.cond_where(
        Condition::all()
            .add(Expr::col((Alias::new(INTEGRATION_TABLE), Alias::new("deleted_at"))).is_null())
            .add(
                Expr::col((Alias::new(INTEGRATION_TABLE), Alias::new("provider")))
                    .eq(Expr::val(GITHUB_PROVIDER)),
            ),
    )
    .order_by(
        (Alias::new(INTEGRATION_TABLE), Alias::new("id")),
        Order::Asc,
    )
    .limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// Live `integrations` row id for `provider="github"`, or `None`.
pub async fn fetch_integration_id_by_provider<'e, E>(
    ex: E,
) -> Result<Option<uuid::Uuid>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&integration_by_provider_sql())
        .fetch_optional(ex)
        .await?;
    row.map(|r| r.try_get("id")).transpose()
}

/// `WorkspaceIntegration.objects.filter(workspace=workspace,
/// integration=integration).first()` (`github.py:106`). Binds
/// `$1 = workspace_id`, `$2 = integration_id`.
pub fn workspace_integration_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(WORKSPACE_INTEGRATION_TABLE.to_owned()));
    sel.column((Alias::new(WORKSPACE_INTEGRATION_TABLE), Alias::new("id")));
    sel.column((
        Alias::new(WORKSPACE_INTEGRATION_TABLE),
        Alias::new("workspace_id"),
    ));
    sel.column((
        Alias::new(WORKSPACE_INTEGRATION_TABLE),
        Alias::new("integration_id"),
    ));
    sel.column((
        Alias::new(WORKSPACE_INTEGRATION_TABLE),
        Alias::new("config"),
    ));
    sel.cond_where(
        Condition::all()
            .add(
                Expr::col((
                    Alias::new(WORKSPACE_INTEGRATION_TABLE),
                    Alias::new("deleted_at"),
                ))
                .is_null(),
            )
            .add(
                Expr::col((
                    Alias::new(WORKSPACE_INTEGRATION_TABLE),
                    Alias::new("workspace_id"),
                ))
                .eq(Expr::cust("$1")),
            )
            .add(
                Expr::col((
                    Alias::new(WORKSPACE_INTEGRATION_TABLE),
                    Alias::new("integration_id"),
                ))
                .eq(Expr::cust("$2")),
            ),
    )
    .order_by(
        (Alias::new(WORKSPACE_INTEGRATION_TABLE), Alias::new("id")),
        Order::Asc,
    )
    .limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// One workspace-integration row: id, workspace, integration, and the
/// `config` JSON the status/repos reads inspect for the token
/// (`github.py:553,581,1000`).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceIntegrationRow {
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub integration_id: uuid::Uuid,
    pub config: serde_json::Value,
}

/// The workspace's github integration row, or `None` (connect 404s,
/// status/disconnect report `{connected: False}`).
pub async fn fetch_workspace_integration<'e, E>(
    ex: E,
    workspace_id: uuid::Uuid,
    integration_id: uuid::Uuid,
) -> Result<Option<WorkspaceIntegrationRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&workspace_integration_sql())
        .bind(workspace_id)
        .bind(integration_id)
        .fetch_optional(ex)
        .await?;
    row.map(|r| {
        Ok(WorkspaceIntegrationRow {
            id: r.try_get("id")?,
            workspace_id: r.try_get("workspace_id")?,
            integration_id: r.try_get("integration_id")?,
            config: r.try_get("config")?,
        })
    })
    .transpose()
}

/// `True` when the workspace-integration `config` carries a non-empty
/// `token` — the `connected` bit of the status read (`github.py:553`).
/// `config` arrives exactly as stored (encrypted at rest); emptiness is
/// checked on the raw value, never decrypted here.
pub fn workspace_integration_is_connected(config: &serde_json::Value) -> bool {
    config
        .get("token")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|t| !t.is_empty())
}

// ---------------------------------------------------------------------------
// Workspace-admin scoping (github.py:131-137, 628-632)
// ---------------------------------------------------------------------------

/// `WorkspaceMember.objects.filter(member=user, role=ADMIN, is_active)
/// .select_related("workspace").order_by("workspace__name")`
/// (`github.py:628-632`, the app-status workspace list). Binds
/// `$1 = member_id`; `ROLE_ADMIN` is the integer 20.
pub fn admin_memberships_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(WORKSPACE_MEMBER_TABLE.to_owned()));
    sel.column((
        Alias::new(WORKSPACE_MEMBER_TABLE),
        Alias::new("workspace_id"),
    ));
    sel.expr_as(
        Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("id"))),
        Alias::new("workspace__id"),
    );
    sel.expr_as(
        Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("slug"))),
        Alias::new("workspace__slug"),
    );
    sel.expr_as(
        Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("name"))),
        Alias::new("workspace__name"),
    );
    sel.join(
        JoinType::InnerJoin,
        Alias::new(WORKSPACE_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((
                Alias::new(WORKSPACE_MEMBER_TABLE),
                Alias::new("workspace_id"),
            ))
            .equals((Alias::new(WORKSPACE_TABLE), Alias::new("id"))),
        ),
    );
    sel.cond_where(
        Condition::all()
            .add(
                Expr::col((Alias::new(WORKSPACE_MEMBER_TABLE), Alias::new("deleted_at"))).is_null(),
            )
            .add(
                Expr::col((Alias::new(WORKSPACE_MEMBER_TABLE), Alias::new("member_id")))
                    .eq(Expr::cust("$1")),
            )
            .add(
                Expr::col((Alias::new(WORKSPACE_MEMBER_TABLE), Alias::new("role")))
                    .eq(Expr::val(ROLE_ADMIN)),
            )
            .add(Expr::col((Alias::new(WORKSPACE_MEMBER_TABLE), Alias::new("is_active"))).eq(true))
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("deleted_at"))).is_null()),
    )
    .order_by(
        (Alias::new(WORKSPACE_TABLE), Alias::new("name")),
        Order::Asc,
    );
    sel.to_string(PostgresQueryBuilder)
}

/// One ADMIN membership with its workspace identity
/// (`github.py:633-646`, the per-workspace payload).
#[derive(Debug, Clone, PartialEq)]
pub struct AdminMembership {
    pub workspace_id: uuid::Uuid,
    pub workspace_slug: String,
    pub workspace_name: String,
}

/// ADMIN workspaces for a user, ordered by workspace name.
pub async fn fetch_admin_memberships<'e, E>(
    ex: E,
    member_id: uuid::Uuid,
) -> Result<Vec<AdminMembership>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(&admin_memberships_sql())
        .bind(member_id)
        .fetch_all(ex)
        .await?;
    rows.iter()
        .map(|r| {
            Ok(AdminMembership {
                workspace_id: r.try_get("workspace_id")?,
                workspace_slug: r.try_get("workspace__slug")?,
                workspace_name: r.try_get("workspace__name")?,
            })
        })
        .collect()
}

/// `_is_workspace_admin(user, workspace)` (`github.py:131-137`):
/// a live ADMIN membership exists. Binds `$1 = member_id`,
/// `$2 = workspace_id`.
pub fn is_workspace_admin_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(WORKSPACE_MEMBER_TABLE.to_owned()));
    sel.expr(Expr::val(1));
    sel.cond_where(
        Condition::all()
            .add(
                Expr::col((Alias::new(WORKSPACE_MEMBER_TABLE), Alias::new("deleted_at"))).is_null(),
            )
            .add(
                Expr::col((Alias::new(WORKSPACE_MEMBER_TABLE), Alias::new("member_id")))
                    .eq(Expr::cust("$1")),
            )
            .add(
                Expr::col((
                    Alias::new(WORKSPACE_MEMBER_TABLE),
                    Alias::new("workspace_id"),
                ))
                .eq(Expr::cust("$2")),
            )
            .add(
                Expr::col((Alias::new(WORKSPACE_MEMBER_TABLE), Alias::new("role")))
                    .eq(Expr::val(ROLE_ADMIN)),
            )
            .add(Expr::col((Alias::new(WORKSPACE_MEMBER_TABLE), Alias::new("is_active"))).eq(true)),
    )
    .limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// Whether the user admins the workspace (install-start, refresh, and
/// callback all gate on this, `github.py:674,708,772`).
pub async fn fetch_is_workspace_admin<'e, E>(
    ex: E,
    member_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&is_workspace_admin_sql())
        .bind(member_id)
        .bind(workspace_id)
        .fetch_optional(ex)
        .await?;
    Ok(row.is_some())
}

// ---------------------------------------------------------------------------
// Project status reads (github.py:1093-1172)
// ---------------------------------------------------------------------------

/// `GithubRepositorySync.objects.filter(project_id=project_id,
/// workspace__slug=slug).select_related("repository").first()`
/// (`github.py:1097-1102`, the legacy-first status branch).
/// Binds `$1 = project_id`, `$2 = workspace slug`.
pub fn repository_sync_for_project_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(github_repository_sync::TABLE.to_owned()));
    for col in github_repository_sync::COLUMNS {
        sel.column((Alias::new(github_repository_sync::TABLE), Alias::new(*col)));
    }
    for col in ["repository_id", "owner", "name", "url"] {
        sel.expr_as(
            Expr::col((Alias::new(GH_REPO_TABLE), Alias::new(col.to_owned()))),
            Alias::new(format!("{GH_REPO_TABLE}__{col}")),
        );
    }
    sel.join(
        JoinType::InnerJoin,
        Alias::new(WORKSPACE_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((
                Alias::new(github_repository_sync::TABLE),
                Alias::new("workspace_id"),
            ))
            .equals((Alias::new(WORKSPACE_TABLE), Alias::new("id"))),
        ),
    );
    sel.join(
        JoinType::InnerJoin,
        Alias::new(GH_REPO_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((
                Alias::new(github_repository_sync::TABLE),
                Alias::new("repository_id"),
            ))
            .equals((Alias::new(GH_REPO_TABLE), Alias::new("id"))),
        ),
    );
    sel.cond_where(
        Condition::all()
            .add(
                Expr::col((
                    Alias::new(github_repository_sync::TABLE),
                    Alias::new("deleted_at"),
                ))
                .is_null(),
            )
            .add(
                Expr::col((
                    Alias::new(github_repository_sync::TABLE),
                    Alias::new("project_id"),
                ))
                .eq(Expr::cust("$1")),
            )
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("slug"))).eq(Expr::cust("$2")))
            .add(Expr::col((Alias::new(GH_REPO_TABLE), Alias::new("deleted_at"))).is_null()),
    )
    .limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// Map one full `github_repository_syncs` row (column names,
/// order-independent).
pub fn map_github_repository_sync_row(row: &PgRow) -> Result<GithubRepositorySync, sqlx::Error> {
    Ok(GithubRepositorySync {
        id: row.try_get("id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        deleted_at: row.try_get("deleted_at")?,
        project_id: row.try_get("project_id")?,
        workspace_id: row.try_get("workspace_id")?,
        repository_id: row.try_get("repository_id")?,
        credentials: row.try_get("credentials")?,
        actor_id: row.try_get("actor_id")?,
        workspace_integration_id: row.try_get("workspace_integration_id")?,
        label_id: row.try_get("label_id")?,
        is_sync_enabled: row.try_get("is_sync_enabled")?,
        last_synced_at: row.try_get("last_synced_at")?,
        last_sync_error: row.try_get("last_sync_error")?,
    })
}

/// Repository identity as the legacy status response renders it
/// (`github.py:1135-1140`): `repository_id`, `owner`, `name`, `url`.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyRepositoryRef {
    pub repository_id: i64,
    pub owner: String,
    pub name: String,
    pub url: Option<String>,
}

/// One legacy sync with its repository (`bound: True` branch,
/// `github.py:1131-1146`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectSyncStatus {
    pub sync: GithubRepositorySync,
    pub repository: LegacyRepositoryRef,
}

/// The project's legacy sync, or `None` (fall through to the
/// `GitRepositoryBinding` fallback, `github.py:1103-1104`).
pub async fn fetch_repository_sync_for_project<'e, E>(
    ex: E,
    project_id: uuid::Uuid,
    workspace_slug: &str,
) -> Result<Option<ProjectSyncStatus>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&repository_sync_for_project_sql())
        .bind(project_id)
        .bind(workspace_slug)
        .fetch_optional(ex)
        .await?;
    row.map(|r| {
        Ok(ProjectSyncStatus {
            sync: map_github_repository_sync_row(&r)?,
            repository: LegacyRepositoryRef {
                repository_id: r.try_get(format!("{GH_REPO_TABLE}__repository_id").as_str())?,
                owner: r.try_get(format!("{GH_REPO_TABLE}__owner").as_str())?,
                name: r.try_get(format!("{GH_REPO_TABLE}__name").as_str())?,
                url: r.try_get(format!("{GH_REPO_TABLE}__url").as_str())?,
            },
        })
    })
    .transpose()
}

/// `GitRepositoryBinding.objects.filter(project_id=project_id,
/// workspace__slug=slug,
/// repository__provider="github").select_related("repository").first()`
/// (`github.py:1104-1112`, the fallback status branch shared by the
/// GET/PATCH/DELETE fallbacks at `:1157-1161,1179-1183`).
/// Binds `$1 = project_id`, `$2 = workspace slug`.
pub fn github_binding_for_project_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(git_repository_binding::TABLE.to_owned()));
    for col in git_repository_binding::COLUMNS {
        sel.column((Alias::new(git_repository_binding::TABLE), Alias::new(*col)));
    }
    for col in [
        "id",
        "provider",
        "host_url",
        "external_id",
        "namespace",
        "name",
        "full_name",
        "web_url",
        "clone_url_http",
        "clone_url_ssh",
        "default_branch",
        "is_private",
    ] {
        sel.expr_as(
            Expr::col((
                Alias::new(git_repository::TABLE.to_owned()),
                Alias::new(col.to_owned()),
            )),
            Alias::new(format!("{}__{col}", git_repository::TABLE)),
        );
    }
    for col in ["id", "status", "last_check_error"] {
        sel.expr_as(
            Expr::col((
                Alias::new(git_provider_account::TABLE.to_owned()),
                Alias::new(col.to_owned()),
            )),
            Alias::new(format!("{}__{col}", git_provider_account::TABLE)),
        );
    }
    sel.join(
        JoinType::InnerJoin,
        Alias::new(WORKSPACE_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((
                Alias::new(git_repository_binding::TABLE),
                Alias::new("workspace_id"),
            ))
            .equals((Alias::new(WORKSPACE_TABLE), Alias::new("id"))),
        ),
    );
    sel.join(
        JoinType::InnerJoin,
        Alias::new(git_repository::TABLE.to_owned()),
        Condition::all().add(
            Expr::col((
                Alias::new(git_repository_binding::TABLE),
                Alias::new("repository_id"),
            ))
            .equals((Alias::new(git_repository::TABLE), Alias::new("id"))),
        ),
    );
    sel.join(
        JoinType::InnerJoin,
        Alias::new(git_provider_account::TABLE.to_owned()),
        Condition::all().add(
            Expr::col((
                Alias::new(git_repository_binding::TABLE),
                Alias::new("provider_account_id"),
            ))
            .equals((Alias::new(git_provider_account::TABLE), Alias::new("id"))),
        ),
    );
    sel.cond_where(
        Condition::all()
            .add(
                Expr::col((
                    Alias::new(git_repository_binding::TABLE),
                    Alias::new("deleted_at"),
                ))
                .is_null(),
            )
            .add(
                Expr::col((
                    Alias::new(git_repository_binding::TABLE),
                    Alias::new("project_id"),
                ))
                .eq(Expr::cust("$1")),
            )
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("slug"))).eq(Expr::cust("$2")))
            .add(Expr::col((Alias::new(git_repository::TABLE), Alias::new("deleted_at"))).is_null())
            .add(
                Expr::col((Alias::new(git_repository::TABLE), Alias::new("provider")))
                    .eq(Expr::val(GITHUB_PROVIDER)),
            )
            .add(
                Expr::col((
                    Alias::new(git_provider_account::TABLE),
                    Alias::new("deleted_at"),
                ))
                .is_null(),
            ),
    )
    .limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// Map one `github_binding_for_project_sql` row, reusing the shared
/// [`super::queries_git`] binding shapes.
pub fn map_github_binding_row(
    row: &PgRow,
) -> Result<super::queries_git::ProjectBinding, sqlx::Error> {
    super::queries_git::map_project_binding_row(row)
}

/// The project's github-provider binding, or `None` (the final
/// `{bound: False}`, `github.py:1113-1114`).
pub async fn fetch_github_binding_for_project<'e, E>(
    ex: E,
    project_id: uuid::Uuid,
    workspace_slug: &str,
) -> Result<Option<super::queries_git::ProjectBinding>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&github_binding_for_project_sql())
        .bind(project_id)
        .bind(workspace_slug)
        .fetch_optional(ex)
        .await?;
    row.map(|r| map_github_binding_row(&r)).transpose()
}

// ---------------------------------------------------------------------------
// Install-session callback read (github.py:751-755)
// ---------------------------------------------------------------------------

/// `GithubAppInstallSession.objects.filter(state=state).select_related(
/// "workspace", "actor").first()` (`github.py:751-753`). `state` is
/// UNIQUE (`github.py:197`), so at most one row. This is the session
/// lookup half: the joined workspace/actor rows the callback then reads
/// (`install_session.workspace.slug`, `actor_id`) resolve through the
/// workspace read in [`super`] plus the FK ids carried on this row.
/// Binds `$1 = state`.
pub fn install_session_by_state_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(github_app_install_session::TABLE.to_owned()));
    for col in github_app_install_session::COLUMNS {
        sel.column((
            Alias::new(github_app_install_session::TABLE),
            Alias::new(*col),
        ));
    }
    sel.cond_where(
        Condition::all()
            .add(
                Expr::col((
                    Alias::new(github_app_install_session::TABLE),
                    Alias::new("deleted_at"),
                ))
                .is_null(),
            )
            .add(
                Expr::col((
                    Alias::new(github_app_install_session::TABLE),
                    Alias::new("state"),
                ))
                .eq(Expr::cust("$1")),
            ),
    )
    .limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// Map one full `github_app_install_sessions` row.
pub fn map_github_app_install_session_row(
    row: &PgRow,
) -> Result<GithubAppInstallSession, sqlx::Error> {
    Ok(GithubAppInstallSession {
        id: row.try_get("id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        deleted_at: row.try_get("deleted_at")?,
        state: row.try_get("state")?,
        workspace_id: row.try_get("workspace_id")?,
        actor_id: row.try_get("actor_id")?,
        installation_id: row.try_get("installation_id")?,
        account_login: row.try_get("account_login")?,
        status: row.try_get("status")?,
        expires_at: row.try_get("expires_at")?,
        completed_at: row.try_get("completed_at")?,
        error: row.try_get("error")?,
    })
}

/// The install session for a callback `state`, or `None` (the
/// `unknown_state` redirect, `github.py:754-755`).
pub async fn fetch_install_session_by_state<'e, E>(
    ex: E,
    state: &str,
) -> Result<Option<GithubAppInstallSession>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&install_session_by_state_sql())
        .bind(state)
        .fetch_optional(ex)
        .await?;
    row.map(|r| map_github_app_install_session_row(&r))
        .transpose()
}

/// The callback expiry predicate: `install_session.expires_at <= now`
/// (`github.py:765`, expired branch). Kept as `<=` exactly as written —
/// see the `BUG (ported)` note at the top of this module.
pub fn install_session_callback_expired(
    expires_at: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    expires_at <= now
}

// ---------------------------------------------------------------------------
// GitHub App auth read-helper shapes (github_app_auth.py, no live calls)
// ---------------------------------------------------------------------------

/// `GITHUB_API_BASE` (`github_app_auth.py:19`).
pub const GITHUB_API_BASE: &str = "https://api.github.com";
/// `GITHUB_WEB_BASE` (`github_app_auth.py:20`).
pub const GITHUB_WEB_BASE: &str = "https://github.com";
/// `DEFAULT_TIMEOUT_SECONDS` (`github_app_auth.py:21`).
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 30;
/// `INSTALLATION_TOKEN_CACHE_PREFIX` (`github_app_auth.py:22`).
pub const INSTALLATION_TOKEN_CACHE_PREFIX: &str = "github_app_installation_token";
/// `Accept` header sent on every GitHub App request
/// (`github_app_auth.py:91,101`).
pub const GITHUB_ACCEPT_HEADER: &str = "application/vnd.github+json";
/// `X-GitHub-Api-Version` header (`github_app_auth.py:92,102`).
pub const GITHUB_API_VERSION_HEADER: &str = "2022-11-28";
/// `User-Agent` header (`github_app_auth.py:93,103`).
pub const GITHUB_USER_AGENT: &str = "pi-dash-github-app";
/// Default installation-token cache TTL, `55 * 60`
/// (`github_app_auth.py:190`).
pub const INSTALLATION_TOKEN_DEFAULT_TTL_SECS: i64 = 55 * 60;
/// Minimum installation-token cache TTL, `max(60, ...)`
/// (`github_app_auth.py:192`; fixture-corrected from `min`).
pub const INSTALLATION_TOKEN_MIN_TTL_SECS: i64 = 60;

/// Seconds subtracted from `now` for the JWT `iat` (clock-drift margin)
/// (`github_app_auth.py:81`).
pub const APP_JWT_IAT_SKEW_SECS: i64 = 60;
/// Seconds added to `now` for the JWT `exp` (`9 * 60`,
/// `github_app_auth.py:82`).
pub const APP_JWT_LIFETIME_SECS: i64 = 9 * 60;

/// `build_app_jwt` claims (`github_app_auth.py:76-85`): `iat = now - 60`,
/// `exp = now + 540`, `iss = app_id`. Key handling (RS256 encode) is
/// transport, not a read shape, and stays out.
#[derive(Debug, Clone, PartialEq)]
pub struct AppJwtClaims {
    pub iat: i64,
    pub exp: i64,
    pub iss: String,
}

/// JWT claims for `now` (unix seconds) and the configured app id.
pub fn build_app_jwt_claims(now_unix: i64, app_id: &str) -> AppJwtClaims {
    AppJwtClaims {
        iat: now_unix - APP_JWT_IAT_SKEW_SECS,
        exp: now_unix + APP_JWT_LIFETIME_SECS,
        iss: app_id.to_owned(),
    }
}

/// `app_headers()` (`github_app_auth.py:88-94`): bearer JWT plus the
/// three fixed GitHub headers, in source order.
pub fn app_headers(jwt: &str) -> Vec<(String, String)> {
    vec![
        ("Authorization".to_owned(), format!("Bearer {jwt}")),
        ("Accept".to_owned(), GITHUB_ACCEPT_HEADER.to_owned()),
        (
            "X-GitHub-Api-Version".to_owned(),
            GITHUB_API_VERSION_HEADER.to_owned(),
        ),
        ("User-Agent".to_owned(), GITHUB_USER_AGENT.to_owned()),
    ]
}

/// `user_headers(token)` (`github_app_auth.py:97-103`): same fixed
/// headers with a user-token bearer.
pub fn user_headers(token: &str) -> Vec<(String, String)> {
    vec![
        ("Authorization".to_owned(), format!("Bearer {token}")),
        ("Accept".to_owned(), GITHUB_ACCEPT_HEADER.to_owned()),
        (
            "X-GitHub-Api-Version".to_owned(),
            GITHUB_API_VERSION_HEADER.to_owned(),
        ),
        ("User-Agent".to_owned(), GITHUB_USER_AGENT.to_owned()),
    ]
}

/// `exchange_user_code(code)` request shape (`github_app_auth.py:112-123`):
/// `POST {WEB}/login/oauth/access_token` with an `Accept: application/json`
/// header, the OAuth client pair plus `code` as form fields, and the
/// 30s default timeout. The response read is
/// `payload.access_token`, else `GithubAppAuthError(error_description or
/// error or "GitHub did not return a user token")` (`:125-131`).
#[derive(Debug, Clone, PartialEq)]
pub struct UserCodeExchange {
    pub url: String,
    pub accept: String,
    pub client_id: String,
    pub client_secret: String,
    pub code: String,
    pub timeout_secs: u64,
}

/// Exchange request for the OAuth client pair and a callback `code`.
pub fn exchange_user_code_shape(
    client_id: &str,
    client_secret: &str,
    code: &str,
) -> UserCodeExchange {
    UserCodeExchange {
        url: format!("{GITHUB_WEB_BASE}/login/oauth/access_token"),
        accept: "application/json".to_owned(),
        client_id: client_id.to_owned(),
        client_secret: client_secret.to_owned(),
        code: code.to_owned(),
        timeout_secs: DEFAULT_TIMEOUT_SECONDS,
    }
}

/// Missing-token error selection from an exchange response payload
/// (`github_app_auth.py:128-130`): `error_description`, else `error`,
/// else the default message.
pub fn exchange_missing_token_error(
    error_description: Option<&str>,
    error: Option<&str>,
) -> String {
    error_description
        .or(error)
        .unwrap_or("GitHub did not return a user token")
        .to_owned()
}

/// `list_user_installations` first-page URL
/// (`github_app_auth.py:136`): `{API}/user/installations?per_page=100`.
pub fn user_installations_first_page_url() -> String {
    format!("{GITHUB_API_BASE}/user/installations?per_page=100")
}

/// Next-page extraction from the `Link` response header
/// (`github_app_auth.py:142-147`): the first comma-part containing
/// `rel="next"` that starts with `<`, unbracketed; `None` otherwise.
pub fn next_installations_page(link_header: &str) -> Option<String> {
    for part in link_header.split(',') {
        let part = part.trim();
        if part.contains("rel=\"next\"") && part.starts_with('<') {
            return part.split('>').next().map(|s| s[1..].to_owned());
        }
    }
    None
}

/// Installation-token cache key
/// (`f"{PREFIX}:{installation_id}"`, `github_app_auth.py:179`).
pub fn installation_token_cache_key(installation_id: i64) -> String {
    format!("{INSTALLATION_TOKEN_CACHE_PREFIX}:{installation_id}")
}

/// Installation-token cache TTL (`github_app_auth.py:189-193`):
/// `max(60, expires_in_secs - 60)`, defaulting to 55 minutes when the
/// response carries no parseable `expires_at`.
pub fn installation_token_ttl_secs(expires_in_secs: Option<i64>) -> i64 {
    match expires_in_secs {
        Some(remaining) => (remaining - 60).max(INSTALLATION_TOKEN_MIN_TTL_SECS),
        None => INSTALLATION_TOKEN_DEFAULT_TTL_SECS,
    }
}

/// `_normalize_private_key` (`github_app_auth.py:54-60`): turn escaped
/// newlines into real ones (single-line PEMs from env/SSM) and strip.
/// Order matters — the escaped-CRLF form first, then escaped-LF — and the
/// scheme literals are plain backslash sequences, matched exactly.
/// Vectors executed in FX-GHA-01. Semantic-trap note: Python
/// `str.strip()` and Rust `str::trim` differ on exotic Unicode
/// whitespace; PEM/config values are ASCII in practice.
pub fn normalize_private_key(value: Option<&str>) -> String {
    value
        .unwrap_or("")
        .replace("\\r\\n", "\n")
        .replace("\\n", "\n")
        .trim()
        .to_owned()
}

/// `parse_github_datetime` (`github_app_auth.py:106-109`): falsy input
/// reads as `None`, else `fromisoformat` after `Z -> +00:00` — and
/// malformed input raises there, so this stays fallible here instead of
/// swallowing to `None`. The Rust parser accepts the strict-RFC3339
/// subset (the fixture vectors); Python `fromisoformat` is more lenient
/// about separators, which never occur in GitHub timestamps.
pub fn parse_github_datetime(
    value: Option<&str>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, chrono::ParseError> {
    let Some(raw) = value.filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    chrono::DateTime::parse_from_rfc3339(&raw.replace('Z', "+00:00"))
        .map(|dt| Some(dt.with_timezone(&chrono::Utc)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_integration_lookup_shape() {
        let sql = workspace_integration_sql();
        assert!(sql.contains(r#"FROM "workspace_integrations""#), "{sql}");
        assert!(sql.contains(r#""workspace_id" = ($1)"#), "{sql}");
        assert!(sql.contains(r#""integration_id" = ($2)"#), "{sql}");
        assert!(sql.contains(r#""config""#), "{sql}");
    }

    #[test]
    fn integration_lookup_is_github_first() {
        let sql = integration_by_provider_sql();
        assert!(sql.contains(r#"FROM "integrations""#), "{sql}");
        assert!(sql.contains("'github'"), "{sql}");
    }

    #[test]
    fn admin_scoping_shape() {
        let sql = admin_memberships_sql();
        assert!(sql.contains(r#"INNER JOIN "workspaces""#), "{sql}");
        assert!(sql.contains(r#""member_id" = ($1)"#), "{sql}");
        assert!(sql.contains("= 20"), "{sql}");
        assert!(sql.contains(r#""is_active" = TRUE"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "workspaces"."name" ASC"#), "{sql}");
        let admin = is_workspace_admin_sql();
        assert!(admin.contains(r#""workspace_id" = ($2)"#), "{admin}");
        assert!(admin.contains("= 20"), "{admin}");
    }

    #[test]
    fn project_status_reads_shape() {
        let sync = repository_sync_for_project_sql();
        assert!(sync.contains(r#"FROM "github_repository_syncs""#), "{sync}");
        assert!(
            sync.contains(r#"INNER JOIN "github_repositories""#),
            "{sync}"
        );
        assert!(sync.contains(r#""project_id" = ($1)"#), "{sync}");
        assert!(sync.contains(r#""workspaces"."slug" = ($2)"#), "{sync}");
        let bind = github_binding_for_project_sql();
        assert!(bind.contains(r#"FROM "git_repository_bindings""#), "{bind}");
        assert!(
            bind.contains(r#""git_repositories"."provider" = 'github'"#),
            "{bind}"
        );
    }

    #[test]
    fn session_lookup_shape() {
        let sql = install_session_by_state_sql();
        assert!(
            sql.contains(r#"FROM "github_app_install_sessions""#),
            "{sql}"
        );
        assert!(sql.contains(r#""state" = ($1)"#), "{sql}");
    }

    #[test]
    fn connected_bit_vectors() {
        assert!(workspace_integration_is_connected(
            &serde_json::json!({"token": "enc"})
        ));
        assert!(!workspace_integration_is_connected(
            &serde_json::json!({"token": ""})
        ));
        assert!(!workspace_integration_is_connected(&serde_json::json!({})));
    }

    #[test]
    fn callback_expiry_uses_lte() {
        let now = chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("ts");
        // BUG (ported): exactly-at-now counts as expired (<=), while the
        // sweep only takes strictly-past (<).
        assert!(install_session_callback_expired(now, now));
        assert!(install_session_callback_expired(
            now - chrono::TimeDelta::seconds(1),
            now
        ));
        assert!(!install_session_callback_expired(
            now + chrono::TimeDelta::seconds(1),
            now
        ));
    }

    #[test]
    fn auth_shape_vectors() {
        // JWT claims (github_app_auth.py:78-84).
        assert_eq!(
            build_app_jwt_claims(1_700_000_000, "123"),
            AppJwtClaims {
                iat: 1_699_999_940,
                exp: 1_700_000_540,
                iss: "123".to_owned()
            }
        );
        // Headers (github_app_auth.py:88-103).
        let h = app_headers("j");
        assert_eq!(h[0], ("Authorization".to_owned(), "Bearer j".to_owned()));
        assert_eq!(h[1].1, "application/vnd.github+json");
        assert_eq!(
            h[2],
            ("X-GitHub-Api-Version".to_owned(), "2022-11-28".to_owned())
        );
        assert_eq!(
            h[3],
            ("User-Agent".to_owned(), "pi-dash-github-app".to_owned())
        );
        let u = user_headers("t");
        assert_eq!(u[0], ("Authorization".to_owned(), "Bearer t".to_owned()));
        assert_eq!(&u[1..], &h[1..]);
        // Exchange shape (github_app_auth.py:112-123).
        let x = exchange_user_code_shape("cid", "sec", "code");
        assert_eq!(x.url, "https://github.com/login/oauth/access_token");
        assert_eq!(x.accept, "application/json");
        assert_eq!(x.timeout_secs, 30);
        // Missing-token error precedence (github_app_auth.py:128-130).
        assert_eq!(exchange_missing_token_error(Some("d"), Some("e")), "d");
        assert_eq!(exchange_missing_token_error(None, Some("e")), "e");
        assert_eq!(
            exchange_missing_token_error(None, None),
            "GitHub did not return a user token"
        );
        // Installations paging (github_app_auth.py:136-147).
        assert_eq!(
            user_installations_first_page_url(),
            "https://api.github.com/user/installations?per_page=100"
        );
        assert_eq!(
            next_installations_page(
                r#"<https://api.github.com/p2>; rel="next", <https://api.github.com/p9>; rel="last""#
            ),
            Some("https://api.github.com/p2".to_owned())
        );
        assert_eq!(next_installations_page(""), None);
        // Cache key + TTL (github_app_auth.py:179,189-193).
        assert_eq!(
            installation_token_cache_key(7),
            "github_app_installation_token:7"
        );
        assert_eq!(installation_token_ttl_secs(None), 3300);
        assert_eq!(installation_token_ttl_secs(Some(3600)), 3540);
        assert_eq!(installation_token_ttl_secs(Some(61)), 60);
        assert_eq!(installation_token_ttl_secs(Some(30)), 60);
        // normalize_private_key vectors (FX-GHA-01).
        assert_eq!(normalize_private_key(Some("LINE1\\nLINE2")), "LINE1\nLINE2");
        assert_eq!(normalize_private_key(None), "");
        assert_eq!(normalize_private_key(Some("a\\r\\nb")), "a\nb");
        // parse_github_datetime vectors (FX-GHA-01).
        assert_eq!(
            parse_github_datetime(Some("2026-09-29T12:00:00Z")).expect("parse"),
            Some(chrono::DateTime::from_timestamp(1_790_683_200, 0).expect("ts"))
        );
        assert_eq!(parse_github_datetime(None).expect("none"), None);
        assert_eq!(parse_github_datetime(Some("")).expect("empty"), None);
        assert!(parse_github_datetime(Some("not-a-date")).is_err());
    }

    // -- live scratch-DB tests (env-gated, see super::tests) --------------

    async fn scratch_pool() -> Option<sqlx::PgPool> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(
                sqlx::PgPool::connect(&url)
                    .await
                    .expect("connect to scratch DATABASE_URL"),
            ),
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                None
            }
        }
    }

    const LIVE_DDL: &[&str] = &[
        "CREATE TEMPORARY TABLE workspaces (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, name VARCHAR(80) NOT NULL, slug VARCHAR(48) NOT NULL UNIQUE)",
        "CREATE TEMPORARY TABLE workspace_members (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, workspace_id UUID NOT NULL, member_id UUID NOT NULL, role SMALLINT NOT NULL, is_active BOOLEAN NOT NULL DEFAULT TRUE)",
        "CREATE TEMPORARY TABLE integrations (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, title VARCHAR(400) NOT NULL, provider VARCHAR(400) NOT NULL UNIQUE, network INTEGER NOT NULL, description JSONB NOT NULL, author VARCHAR(400) NOT NULL, webhook_url TEXT NOT NULL, webhook_secret TEXT NOT NULL, redirect_url TEXT NOT NULL, metadata JSONB NOT NULL, verified BOOLEAN NOT NULL, avatar_url TEXT)",
        "CREATE TEMPORARY TABLE workspace_integrations (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, workspace_id UUID NOT NULL, actor_id UUID NOT NULL, integration_id UUID NOT NULL, api_token_id UUID NOT NULL, metadata JSONB NOT NULL, config JSONB NOT NULL)",
        "CREATE TEMPORARY TABLE github_repositories (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, project_id UUID NOT NULL, workspace_id UUID NOT NULL, name VARCHAR(500) NOT NULL, url VARCHAR(500), config JSONB NOT NULL, repository_id BIGINT NOT NULL, owner VARCHAR(500) NOT NULL)",
        "CREATE TEMPORARY TABLE github_repository_syncs (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, project_id UUID NOT NULL, workspace_id UUID NOT NULL, repository_id UUID NOT NULL, credentials JSONB NOT NULL, actor_id UUID NOT NULL, workspace_integration_id UUID NOT NULL, label_id UUID, is_sync_enabled BOOLEAN NOT NULL, last_synced_at TIMESTAMPTZ, last_sync_error TEXT NOT NULL)",
        "CREATE TEMPORARY TABLE git_repositories (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, provider VARCHAR(32) NOT NULL, host_url VARCHAR(500) NOT NULL, external_id VARCHAR(255) NOT NULL, namespace VARCHAR(500) NOT NULL, name VARCHAR(500) NOT NULL, full_name VARCHAR(1000) NOT NULL, web_url VARCHAR(1000) NOT NULL, clone_url_http VARCHAR(1000) NOT NULL, clone_url_ssh VARCHAR(1000) NOT NULL, default_branch VARCHAR(255) NOT NULL, is_private BOOLEAN NOT NULL, metadata JSONB NOT NULL)",
        "CREATE TEMPORARY TABLE git_provider_accounts (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, workspace_id UUID NOT NULL, provider VARCHAR(32) NOT NULL, host_url VARCHAR(500) NOT NULL, auth_type VARCHAR(32) NOT NULL, external_account_id VARCHAR(255) NOT NULL, external_account_login VARCHAR(255) NOT NULL, display_name VARCHAR(255) NOT NULL, capabilities JSONB NOT NULL, credential_config JSONB NOT NULL, workspace_integration_id UUID, status VARCHAR(16) NOT NULL, verified_at TIMESTAMPTZ, last_check_error TEXT NOT NULL, metadata JSONB NOT NULL)",
        "CREATE TEMPORARY TABLE git_repository_bindings (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, project_id UUID NOT NULL, workspace_id UUID NOT NULL, repository_id UUID NOT NULL, provider_account_id UUID NOT NULL, actor_id UUID NOT NULL, is_sync_enabled BOOLEAN NOT NULL, clone_auth_mode VARCHAR(32) NOT NULL, last_synced_at TIMESTAMPTZ, last_sync_error TEXT NOT NULL, metadata JSONB NOT NULL)",
        "CREATE TEMPORARY TABLE github_app_install_sessions (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, state VARCHAR(128) NOT NULL UNIQUE, workspace_id UUID NOT NULL, actor_id UUID NOT NULL, installation_id BIGINT, account_login VARCHAR(255) NOT NULL, status VARCHAR(16) NOT NULL, expires_at TIMESTAMPTZ NOT NULL, completed_at TIMESTAMPTZ, error TEXT NOT NULL)",
    ];

    async fn live_tx(pool: &sqlx::PgPool) -> sqlx::Transaction<'_, sqlx::Postgres> {
        let mut tx = pool.begin().await.expect("begin scratch tx");
        for ddl in LIVE_DDL {
            sqlx::query(ddl)
                .execute(&mut *tx)
                .await
                .expect("create temp table");
        }
        tx
    }

    fn live_uuid(suffix: u8) -> uuid::Uuid {
        uuid::Uuid::parse_str(&format!("44444444-4444-4444-4444-4444444444{suffix:02}"))
            .expect("fixed test uuid")
    }

    fn live_ts(secs: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(secs, 0).expect("fixed test timestamp")
    }

    /// FX-GHA-02 replay: integration lookup, wi read + connected bit,
    /// ADMIN membership scoping + ordering, admin predicate.
    #[tokio::test]
    async fn live_workspace_reads() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let ts = live_ts(1_700_000_000);
        let (ws, other, integ, wi, admin, member, guest) = (
            live_uuid(1),
            live_uuid(2),
            live_uuid(3),
            live_uuid(4),
            live_uuid(5),
            live_uuid(6),
            live_uuid(7),
        );
        for (id, slug, name) in [(ws, "acme", "Acme"), (other, "zeta", "Zeta")] {
            sqlx::query(
                "INSERT INTO workspaces (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, name, slug) VALUES ($1, $2, $2, NULL, NULL, NULL, $3, $4)",
            )
            .bind(id).bind(ts).bind(name).bind(slug)
            .execute(&mut *tx).await.expect("seed ws");
        }
        sqlx::query(
            "INSERT INTO integrations (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, title, provider, network, description, author, webhook_url, webhook_secret, redirect_url, metadata, verified, avatar_url) VALUES ($1, $2, $2, NULL, NULL, NULL, 'GitHub', 'github', 1, '{}', '', '', '', '', '{}', TRUE, NULL)",
        )
        .bind(integ).bind(ts)
        .execute(&mut *tx).await.expect("seed integration");
        sqlx::query(
            "INSERT INTO workspace_integrations (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, actor_id, integration_id, api_token_id, metadata, config) VALUES ($1, $2, $2, NULL, NULL, NULL, $3, $4, $5, $4, '{}', $6)",
        )
        .bind(wi).bind(ts).bind(ws).bind(admin).bind(integ)
        .bind(serde_json::json!({"token": "enc", "github_user_login": "octo"}))
        .execute(&mut *tx).await.expect("seed wi");
        // admin of both (member rows named out of order to prove ORDER BY name).
        for (mid, w, m, role, active) in [
            (live_uuid(11), other, admin, 20i16, true),
            (live_uuid(12), ws, admin, 20i16, true),
            (live_uuid(13), ws, member, 15i16, true),
            (live_uuid(14), ws, guest, 20i16, false),
        ] {
            sqlx::query(
                "INSERT INTO workspace_members (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, member_id, role, is_active) VALUES ($1, $2, $2, NULL, NULL, NULL, $3, $4, $5, $6)",
            )
            .bind(mid).bind(ts).bind(w).bind(m).bind(role).bind(active)
            .execute(&mut *tx).await.expect("seed member");
        }

        assert_eq!(
            fetch_integration_id_by_provider(&mut *tx)
                .await
                .expect("integration"),
            Some(integ)
        );
        let wi_row = fetch_workspace_integration(&mut *tx, ws, integ)
            .await
            .expect("wi")
            .expect("a row");
        assert_eq!(wi_row.id, wi);
        assert!(workspace_integration_is_connected(&wi_row.config));
        assert_eq!(
            fetch_workspace_integration(&mut *tx, other, integ)
                .await
                .expect("wi"),
            None
        );
        // ADMIN memberships only, ordered by workspace name (Acme < Zeta).
        let memberships = fetch_admin_memberships(&mut *tx, admin)
            .await
            .expect("memberships");
        assert_eq!(
            memberships
                .iter()
                .map(|m| m.workspace_slug.clone())
                .collect::<Vec<_>>(),
            vec!["acme".to_string(), "zeta".to_string()]
        );
        assert!(memberships
            .iter()
            .all(|m| m.workspace_name == "Acme" || m.workspace_name == "Zeta"));
        assert!(fetch_admin_memberships(&mut *tx, member)
            .await
            .expect("m")
            .is_empty());
        assert!(fetch_admin_memberships(&mut *tx, guest)
            .await
            .expect("g")
            .is_empty());
        assert!(fetch_is_workspace_admin(&mut *tx, admin, ws)
            .await
            .expect("gate"));
        assert!(!fetch_is_workspace_admin(&mut *tx, member, ws)
            .await
            .expect("gate"));
        assert!(!fetch_is_workspace_admin(&mut *tx, guest, ws)
            .await
            .expect("gate"));
        assert!(!fetch_is_workspace_admin(&mut *tx, admin, live_uuid(9))
            .await
            .expect("gate"));
        tx.rollback().await.expect("rollback");
    }

    /// FX-GHA-03 replay: legacy-first project status, then the github
    /// binding fallback; FX-GHA-01 replay: session-by-state read.
    #[tokio::test]
    async fn live_project_status_and_session() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let ts = live_ts(1_700_000_000);
        let (ws, proj_sync, proj_bind, proj_none) =
            (live_uuid(1), live_uuid(2), live_uuid(3), live_uuid(4));
        let (repo_legacy, sync, repo_git, acct, bind) = (
            live_uuid(11),
            live_uuid(12),
            live_uuid(13),
            live_uuid(14),
            live_uuid(15),
        );
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, name, slug) VALUES ($1, $2, $2, NULL, NULL, NULL, 'n', 'acme')",
        )
        .bind(ws).bind(ts)
        .execute(&mut *tx).await.expect("seed ws");
        // Legacy side: github_repositories + github_repository_syncs.
        sqlx::query(
            "INSERT INTO github_repositories (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, name, url, config, repository_id, owner) VALUES ($1, $2, $2, NULL, NULL, NULL, $3, $4, 'repo', 'https://github.com/o/r', '{}', 4242, 'o')",
        )
        .bind(repo_legacy).bind(ts).bind(proj_sync).bind(ws)
        .execute(&mut *tx).await.expect("seed gh repo");
        sqlx::query(
            "INSERT INTO github_repository_syncs (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, repository_id, credentials, actor_id, workspace_integration_id, label_id, is_sync_enabled, last_synced_at, last_sync_error) VALUES ($1, $2, $2, NULL, NULL, NULL, $3, $4, $5, '{}', $4, $4, NULL, TRUE, NULL, '')",
        )
        .bind(sync).bind(ts).bind(proj_sync).bind(ws).bind(repo_legacy)
        .execute(&mut *tx).await.expect("seed sync");
        // Fallback side: git chain for a github provider repo.
        sqlx::query(
            "INSERT INTO git_repositories (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, provider, host_url, external_id, namespace, name, full_name, web_url, clone_url_http, clone_url_ssh, default_branch, is_private, metadata) VALUES ($1, $2, $2, NULL, NULL, NULL, 'github', 'https://github.com', '77', 'o', 'g', 'o/g', 'https://github.com/o/g', '', '', 'main', TRUE, '{}')",
        )
        .bind(repo_git).bind(ts)
        .execute(&mut *tx).await.expect("seed git repo");
        sqlx::query(
            "INSERT INTO git_provider_accounts (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, provider, host_url, auth_type, external_account_id, external_account_login, display_name, capabilities, credential_config, workspace_integration_id, status, verified_at, last_check_error, metadata) VALUES ($1, $2, $2, NULL, NULL, NULL, $3, 'github', 'https://github.com', 'pat', 'e', 'l', 'a', '{}', '{}', NULL, 'degraded', NULL, 'stale', '{}')",
        )
        .bind(acct).bind(ts).bind(ws)
        .execute(&mut *tx).await.expect("seed account");
        sqlx::query(
            "INSERT INTO git_repository_bindings (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, repository_id, provider_account_id, actor_id, is_sync_enabled, clone_auth_mode, last_synced_at, last_sync_error, metadata) VALUES ($1, $2, $2, NULL, NULL, NULL, $3, $4, $5, $6, $4, FALSE, 'runner_managed', NULL, 'e', '{}')",
        )
        .bind(bind).bind(ts).bind(proj_bind).bind(ws).bind(repo_git).bind(acct)
        .execute(&mut *tx).await.expect("seed binding");
        // Install session (FX-GHA-01 read).
        sqlx::query(
            "INSERT INTO github_app_install_sessions (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, state, workspace_id, actor_id, installation_id, account_login, status, expires_at, completed_at, error) VALUES ($1, $2, $2, NULL, NULL, NULL, 'st_abc', $3, $3, NULL, '', 'started', $4, NULL, '')",
        )
        .bind(live_uuid(21)).bind(ts).bind(ws).bind(live_ts(1_700_090_000))
        .execute(&mut *tx).await.expect("seed session");

        // Legacy-first hit.
        let hit = fetch_repository_sync_for_project(&mut *tx, proj_sync, "acme")
            .await
            .expect("sync")
            .expect("a row");
        assert_eq!(hit.sync.id, sync);
        assert!(hit.sync.is_sync_enabled);
        assert_eq!(hit.repository.repository_id, 4242);
        assert_eq!(hit.repository.owner, "o");
        assert_eq!(
            hit.repository.url.as_deref(),
            Some("https://github.com/o/r")
        );
        // No legacy row for the bound project -> fallback None on the sync read...
        assert_eq!(
            fetch_repository_sync_for_project(&mut *tx, proj_bind, "acme")
                .await
                .expect("sync"),
            None
        );
        // ...but the github binding fallback hits (private repo preserved).
        let fb = fetch_github_binding_for_project(&mut *tx, proj_bind, "acme")
            .await
            .expect("binding")
            .expect("a row");
        assert_eq!(fb.binding.id, bind);
        assert_eq!(fb.repository.full_name, "o/g");
        assert!(fb.repository.is_private);
        assert_eq!(fb.provider_account.status, "degraded");
        assert_eq!(fb.provider_account.last_check_error, "stale");
        // Nothing bound at all -> both miss (the {bound: False} branch).
        assert_eq!(
            fetch_repository_sync_for_project(&mut *tx, proj_none, "acme")
                .await
                .expect("s"),
            None
        );
        assert_eq!(
            fetch_github_binding_for_project(&mut *tx, proj_none, "acme")
                .await
                .expect("b"),
            None
        );
        // Session read.
        let sess = fetch_install_session_by_state(&mut *tx, "st_abc")
            .await
            .expect("session")
            .expect("a row");
        assert_eq!(sess.status, "started");
        assert_eq!(
            fetch_install_session_by_state(&mut *tx, "nope")
                .await
                .expect("session"),
            None
        );
        tx.rollback().await.expect("rollback");
    }
}
