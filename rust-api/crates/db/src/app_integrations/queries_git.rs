//! Git-integration query reads (D-33, stage 5).
//!
//! Ports the database read shapes behind
//! `apps/api/pi_dash/app/views/integration/git.py:50-175` and the
//! `get_binding` / `_provider_account_queryset` helpers in
//! `apps/api/pi_dash/integrations/git/services.py:63-70,353-358`.
//! Fixtures FX-GIT-01, FX-GIT-02
//! (`rust-api/fixtures/app_integrations/fx-git-0{1,2}-*.json`).
//!
//! Dynamic statements are sea-query builders; fixed reads are string
//! constants executed with runtime `sqlx::query` (no `query!` macros:
//! there is no build-time database, same as the merged `license/queries`
//! precedent). Every executor is generic over `sqlx::Executor`. Row
//! structs reuse the D-05 models (`crate::integrations::git_models`,
//! PIDASHCONV-42, Done) — this module ports queries only, never models.
//!
//! SQL semantics are Django's, quirks included (translate, don't
//! redesign):
//!
//! * The default manager's `deleted_at IS NULL` (`db/mixins.py:57-58`)
//!   applies to every table in every read, including the joined
//!   `workspaces` row on `workspace__slug` traversals.
//! * `order_by("provider", "host_url", "display_name")` is three
//!   ascending keys (`git.py:61-63`).
//! * The `page` rule is `max(1, int(raw or "1"))` with `ValueError -> 1`
//!   (`git.py:121-124`, identical in `github.py:585-588`).
//! * `normalize_host_url` strips, drops trailing `/`, and prepends
//!   `https://` only when no `http(s)://` scheme is present
//!   (`services.py:43-47`; vectors executed in FX-GIT-01).

use sea_query::{Alias, Condition, Expr, JoinType, Order, Query, SelectStatement};
use sqlx::postgres::PgRow;
use sqlx::Row;

use super::WORKSPACE_TABLE;
use crate::integrations::git_models::{
    git_provider_account::{self, GitProviderAccount},
    git_repository,
    git_repository_binding::{self, GitRepositoryBinding},
};

/// `GitProviderAccount` table shorthand.
const ACCOUNT_TABLE: &str = git_provider_account::TABLE;
/// `GitRepositoryBinding` table shorthand.
const BINDING_TABLE: &str = git_repository_binding::TABLE;
/// Live account statuses admitted by `_provider_account_queryset`
/// (`services.py:63-70`).
pub const RESOLVABLE_STATUSES: &[&str] = &[
    git_provider_account::STATUS_CONNECTED,
    git_provider_account::STATUS_DEGRADED,
];

// ---------------------------------------------------------------------------
// Shared column helpers
// ---------------------------------------------------------------------------

/// `SELECT` every `GitProviderAccount` column, table-qualified.
fn select_account_columns(sel: &mut SelectStatement) {
    for col in git_provider_account::COLUMNS {
        sel.column((Alias::new(ACCOUNT_TABLE), Alias::new(*col)));
    }
}

/// Map one full `git_provider_accounts` row (column names,
/// order-independent).
pub fn map_git_provider_account_row(row: &PgRow) -> Result<GitProviderAccount, sqlx::Error> {
    Ok(GitProviderAccount {
        id: row.try_get("id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        deleted_at: row.try_get("deleted_at")?,
        workspace_id: row.try_get("workspace_id")?,
        provider: row.try_get("provider")?,
        host_url: row.try_get("host_url")?,
        auth_type: row.try_get("auth_type")?,
        external_account_id: row.try_get("external_account_id")?,
        external_account_login: row.try_get("external_account_login")?,
        display_name: row.try_get("display_name")?,
        capabilities: row.try_get("capabilities")?,
        credential_config: row.try_get("credential_config")?,
        workspace_integration_id: row.try_get("workspace_integration_id")?,
        status: row.try_get("status")?,
        verified_at: row.try_get("verified_at")?,
        last_check_error: row.try_get("last_check_error")?,
        metadata: row.try_get("metadata")?,
    })
}

// ---------------------------------------------------------------------------
// Provider-account list (git.py:57-67)
// ---------------------------------------------------------------------------

/// `GitProviderAccount.objects.filter(workspace=workspace).order_by(
/// "provider", "host_url", "display_name")` (`git.py:61-63`).
/// Binds `$1 = workspace_id`.
pub fn provider_account_list_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(ACCOUNT_TABLE.to_owned()));
    select_account_columns(&mut sel);
    sel.cond_where(
        Condition::all()
            .add(Expr::col((Alias::new(ACCOUNT_TABLE), Alias::new("deleted_at"))).is_null())
            .add(
                Expr::col((Alias::new(ACCOUNT_TABLE), Alias::new("workspace_id")))
                    .eq(Expr::cust("$1")),
            ),
    )
    .order_by(
        (Alias::new(ACCOUNT_TABLE), Alias::new("provider")),
        Order::Asc,
    )
    .order_by(
        (Alias::new(ACCOUNT_TABLE), Alias::new("host_url")),
        Order::Asc,
    )
    .order_by(
        (Alias::new(ACCOUNT_TABLE), Alias::new("display_name")),
        Order::Asc,
    );
    sel.to_string(PostgresQueryBuilder)
}

/// Ordered provider accounts for one workspace
/// (`git.py:64-67`, `{"accounts": [...]}`).
pub async fn fetch_provider_account_list<'e, E>(
    ex: E,
    workspace_id: uuid::Uuid,
) -> Result<Vec<GitProviderAccount>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(&provider_account_list_sql())
        .bind(workspace_id)
        .fetch_all(ex)
        .await?;
    rows.iter().map(map_git_provider_account_row).collect()
}

// ---------------------------------------------------------------------------
// Provider-account detail (git.py:97-101, delete path :103-114 reads :105)
// ---------------------------------------------------------------------------

/// `get_object_or_404(GitProviderAccount, id=account_id,
/// workspace__slug=slug)` (`git.py:100,105`): the `workspace__slug`
/// traversal inner-joins `workspaces` (non-nullable FK) and applies the
/// default manager's `deleted_at IS NULL` on both tables.
/// Binds `$1 = account_id`, `$2 = workspace slug`.
pub fn provider_account_detail_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(ACCOUNT_TABLE.to_owned()));
    select_account_columns(&mut sel);
    sel.join(
        JoinType::InnerJoin,
        Alias::new(WORKSPACE_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(ACCOUNT_TABLE), Alias::new("workspace_id")))
                .equals((Alias::new(WORKSPACE_TABLE), Alias::new("id"))),
        ),
    );
    sel.cond_where(
        Condition::all()
            .add(Expr::col((Alias::new(ACCOUNT_TABLE), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((Alias::new(ACCOUNT_TABLE), Alias::new("id"))).eq(Expr::cust("$1")))
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("slug"))).eq(Expr::cust("$2"))),
    )
    .limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// One provider account by id scoped to a workspace slug, or `None`
/// (the view's 404 branch).
pub async fn fetch_provider_account_detail<'e, E>(
    ex: E,
    account_id: uuid::Uuid,
    workspace_slug: &str,
) -> Result<Option<GitProviderAccount>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&provider_account_detail_sql())
        .bind(account_id)
        .bind(workspace_slug)
        .fetch_optional(ex)
        .await?;
    row.map(|r| map_git_provider_account_row(&r)).transpose()
}

// ---------------------------------------------------------------------------
// Project binding read (services.py:353-358, git.py:132-138)
// ---------------------------------------------------------------------------

/// `GitRepository` table shorthand.
const REPO_TABLE: &str = git_repository::TABLE;

/// Repository columns `serialize_binding` needs, fetched under the
/// `git_repositories__` alias prefix (see the module docs for why the
/// join is aliased while Django maps positionally).
const BINDING_REPO_COLS: &[&str] = &[
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
];

/// Account columns `serialize_binding` needs for the `degraded` derivation
/// (`provider_account.status != CONNECTED`, `services.py:280-295`).
const BINDING_ACCOUNT_COLS: &[&str] = &["id", "status", "last_check_error"];

/// Repository fields read through a project binding
/// (`serialize_binding` + `serialize_repository`, `services.py:232-295`).
#[derive(Debug, Clone, PartialEq)]
pub struct BoundRepository {
    pub id: uuid::Uuid,
    pub provider: String,
    pub host_url: String,
    pub external_id: String,
    pub namespace: String,
    pub name: String,
    pub full_name: String,
    pub web_url: String,
    pub clone_url_http: String,
    pub clone_url_ssh: String,
    pub default_branch: String,
    pub is_private: bool,
}

/// Account fields read through a project binding (the `degraded` inputs).
#[derive(Debug, Clone, PartialEq)]
pub struct BoundAccount {
    pub id: uuid::Uuid,
    pub status: String,
    pub last_check_error: String,
}

/// One project binding with its `select_related` repository and provider
/// account (`services.py:353-358`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectBinding {
    pub binding: GitRepositoryBinding,
    pub repository: BoundRepository,
    pub provider_account: BoundAccount,
}

/// Alias a joined-table column as `<table>__<col>`.
fn select_aliased(sel: &mut SelectStatement, table: &str, cols: &[&str]) {
    for col in cols {
        sel.expr_as(
            Expr::col((Alias::new(table.to_owned()), Alias::new((*col).to_owned()))),
            Alias::new(format!("{table}__{col}")),
        );
    }
}

/// `GitRepositoryBinding.objects.filter(project_id=project_id,
/// workspace__slug=slug).select_related("repository",
/// "provider_account").first()` (`services.py:353-358`).
/// Binds `$1 = project_id`, `$2 = workspace slug`.
pub fn binding_for_project_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(BINDING_TABLE.to_owned()));
    for col in git_repository_binding::COLUMNS {
        sel.column((Alias::new(BINDING_TABLE), Alias::new(*col)));
    }
    select_aliased(&mut sel, REPO_TABLE, BINDING_REPO_COLS);
    select_aliased(&mut sel, ACCOUNT_TABLE, BINDING_ACCOUNT_COLS);
    sel.join(
        JoinType::InnerJoin,
        Alias::new(WORKSPACE_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(BINDING_TABLE), Alias::new("workspace_id")))
                .equals((Alias::new(WORKSPACE_TABLE), Alias::new("id"))),
        ),
    );
    sel.join(
        JoinType::InnerJoin,
        Alias::new(REPO_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(BINDING_TABLE), Alias::new("repository_id")))
                .equals((Alias::new(REPO_TABLE), Alias::new("id"))),
        ),
    );
    sel.join(
        JoinType::InnerJoin,
        Alias::new(ACCOUNT_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((Alias::new(BINDING_TABLE), Alias::new("provider_account_id")))
                .equals((Alias::new(ACCOUNT_TABLE), Alias::new("id"))),
        ),
    );
    sel.cond_where(
        Condition::all()
            .add(Expr::col((Alias::new(BINDING_TABLE), Alias::new("deleted_at"))).is_null())
            .add(
                Expr::col((Alias::new(BINDING_TABLE), Alias::new("project_id")))
                    .eq(Expr::cust("$1")),
            )
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("slug"))).eq(Expr::cust("$2")))
            .add(Expr::col((Alias::new(REPO_TABLE), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((Alias::new(ACCOUNT_TABLE), Alias::new("deleted_at"))).is_null()),
    )
    .limit(1);
    sel.to_string(PostgresQueryBuilder)
}

fn aliased(row: &PgRow, table: &str, col: &str) -> Result<String, sqlx::Error> {
    row.try_get(format!("{table}__{col}").as_str())
}

fn aliased_uuid(row: &PgRow, table: &str, col: &str) -> Result<uuid::Uuid, sqlx::Error> {
    row.try_get(format!("{table}__{col}").as_str())
}

fn aliased_bool(row: &PgRow, table: &str, col: &str) -> Result<bool, sqlx::Error> {
    row.try_get(format!("{table}__{col}").as_str())
}

/// Map one `binding_for_project_sql` row.
pub fn map_project_binding_row(row: &PgRow) -> Result<ProjectBinding, sqlx::Error> {
    Ok(ProjectBinding {
        binding: GitRepositoryBinding {
            id: row.try_get("id")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            created_by_id: row.try_get("created_by_id")?,
            updated_by_id: row.try_get("updated_by_id")?,
            deleted_at: row.try_get("deleted_at")?,
            project_id: row.try_get("project_id")?,
            workspace_id: row.try_get("workspace_id")?,
            repository_id: row.try_get("repository_id")?,
            provider_account_id: row.try_get("provider_account_id")?,
            actor_id: row.try_get("actor_id")?,
            is_sync_enabled: row.try_get("is_sync_enabled")?,
            clone_auth_mode: row.try_get("clone_auth_mode")?,
            last_synced_at: row.try_get("last_synced_at")?,
            last_sync_error: row.try_get("last_sync_error")?,
            metadata: row.try_get("metadata")?,
        },
        repository: BoundRepository {
            id: aliased_uuid(row, REPO_TABLE, "id")?,
            provider: aliased(row, REPO_TABLE, "provider")?,
            host_url: aliased(row, REPO_TABLE, "host_url")?,
            external_id: aliased(row, REPO_TABLE, "external_id")?,
            namespace: aliased(row, REPO_TABLE, "namespace")?,
            name: aliased(row, REPO_TABLE, "name")?,
            full_name: aliased(row, REPO_TABLE, "full_name")?,
            web_url: aliased(row, REPO_TABLE, "web_url")?,
            clone_url_http: aliased(row, REPO_TABLE, "clone_url_http")?,
            clone_url_ssh: aliased(row, REPO_TABLE, "clone_url_ssh")?,
            default_branch: aliased(row, REPO_TABLE, "default_branch")?,
            is_private: aliased_bool(row, REPO_TABLE, "is_private")?,
        },
        provider_account: BoundAccount {
            id: aliased_uuid(row, ACCOUNT_TABLE, "id")?,
            status: aliased(row, ACCOUNT_TABLE, "status")?,
            last_check_error: aliased(row, ACCOUNT_TABLE, "last_check_error")?,
        },
    })
}

/// The project's binding (`None` renders `{"bound": False}`,
/// `git.py:136-137`).
pub async fn fetch_binding_for_project<'e, E>(
    ex: E,
    project_id: uuid::Uuid,
    workspace_slug: &str,
) -> Result<Option<ProjectBinding>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(&binding_for_project_sql())
        .bind(project_id)
        .bind(workspace_slug)
        .fetch_optional(ex)
        .await?;
    row.map(|r| map_project_binding_row(&r)).transpose()
}

// ---------------------------------------------------------------------------
// Bind-resolution queryset (services.py:63-70,110-131)
// ---------------------------------------------------------------------------

/// `_provider_account_queryset(workspace, provider, host_url)`
/// (`services.py:63-70`): live accounts for one host. The caller passes
/// `host_url` already through [`normalize_host_url`]. Binds
/// `$1 = workspace_id`, `$2 = provider`, `$3 = host_url`.
pub fn provider_account_resolution_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = Query::select();
    sel.from(Alias::new(ACCOUNT_TABLE.to_owned()));
    select_account_columns(&mut sel);
    sel.cond_where(
        Condition::all()
            .add(Expr::col((Alias::new(ACCOUNT_TABLE), Alias::new("deleted_at"))).is_null())
            .add(
                Expr::col((Alias::new(ACCOUNT_TABLE), Alias::new("workspace_id")))
                    .eq(Expr::cust("$1")),
            )
            .add(
                Expr::col((Alias::new(ACCOUNT_TABLE), Alias::new("provider"))).eq(Expr::cust("$2")),
            )
            .add(
                Expr::col((Alias::new(ACCOUNT_TABLE), Alias::new("host_url"))).eq(Expr::cust("$3")),
            )
            .add(
                Expr::col((Alias::new(ACCOUNT_TABLE), Alias::new("status")))
                    .is_in(vec![Expr::cust("$4"), Expr::cust("$5")]),
            ),
    )
    .order_by((Alias::new(ACCOUNT_TABLE), Alias::new("id")), Order::Asc);
    sel.to_string(PostgresQueryBuilder)
}

/// Candidate accounts for binding a repository host: explicit
/// `provider_account_id` narrows to one id (404 when absent from this
/// set); no id with 0 rows raises 409 Required and >1 rows raises 409
/// Ambiguous (`services.py:110-131`). The count/branching itself is
/// handler logic; this is the shared read.
pub async fn fetch_provider_account_resolution<'e, E>(
    ex: E,
    workspace_id: uuid::Uuid,
    provider: &str,
    host_url: &str,
) -> Result<Vec<GitProviderAccount>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(&provider_account_resolution_sql())
        .bind(workspace_id)
        .bind(provider)
        .bind(host_url)
        .bind(git_provider_account::STATUS_CONNECTED)
        .bind(git_provider_account::STATUS_DEGRADED)
        .fetch_all(ex)
        .await?;
    rows.iter().map(map_git_provider_account_row).collect()
}

// ---------------------------------------------------------------------------
// Pure read-shape helpers
// ---------------------------------------------------------------------------

/// `normalize_host_url` (`services.py:43-47`): strip, drop trailing `/`,
/// prepend `https://` only when the remainder does not start with the
/// lowercase `http://`/`https://` literals (the check is case-sensitive).
/// Vectors executed in FX-GIT-01.
pub fn normalize_host_url(raw: &str) -> String {
    let stripped = raw.trim().trim_end_matches('/');
    if stripped.is_empty() {
        return String::new();
    }
    let prefixed = if stripped.starts_with("http://") || stripped.starts_with("https://") {
        stripped.to_owned()
    } else {
        format!("https://{stripped}")
    };
    prefixed.trim_end_matches('/').to_owned()
}

/// The `page` rule shared by the git repos read (`git.py:121-124`) and
/// the github repos read (`github.py:585-588`): `max(1, int(raw or "1"))`
/// with `ValueError -> 1`.
pub fn parse_page_param(raw: Option<&str>) -> i64 {
    raw.unwrap_or("1").trim().parse::<i64>().unwrap_or(1).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_orders_by_provider_host_display_name() {
        let sql = provider_account_list_sql();
        assert!(sql.contains(r#"FROM "git_provider_accounts""#), "{sql}");
        assert!(
            sql.contains(r#"ORDER BY "git_provider_accounts"."provider" ASC, "git_provider_accounts"."host_url" ASC, "git_provider_accounts"."display_name" ASC"#),
            "{sql}"
        );
        assert!(sql.contains(r#""workspace_id" = ($1)"#), "{sql}");
    }

    #[test]
    fn detail_scopes_id_to_workspace_slug() {
        let sql = provider_account_detail_sql();
        assert!(sql.contains(r#"INNER JOIN "workspaces""#), "{sql}");
        assert!(
            sql.contains(r#""git_provider_accounts"."id" = ($1)"#),
            "{sql}"
        );
        assert!(sql.contains(r#""workspaces"."slug" = ($2)"#), "{sql}");
        assert_eq!(sql.matches("deleted_at\" IS NULL").count(), 2, "{sql}");
    }

    #[test]
    fn binding_joins_repo_account_and_workspace() {
        let sql = binding_for_project_sql();
        assert!(sql.contains(r#"FROM "git_repository_bindings""#), "{sql}");
        assert!(sql.contains(r#"INNER JOIN "git_repositories""#), "{sql}");
        assert!(
            sql.contains(r#"INNER JOIN "git_provider_accounts""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""git_repository_bindings"."project_id" = ($1)"#),
            "{sql}"
        );
        assert!(sql.contains(r#""workspaces"."slug" = ($2)"#), "{sql}");
        assert!(sql.contains(r#""git_repositories__full_name""#), "{sql}");
        assert!(sql.contains(r#""git_provider_accounts__status""#), "{sql}");
    }

    #[test]
    fn resolution_filters_host_and_live_statuses() {
        let sql = provider_account_resolution_sql();
        assert!(sql.contains(r#""provider" = ($2)"#), "{sql}");
        assert!(sql.contains(r#""host_url" = ($3)"#), "{sql}");
        assert!(sql.contains("IN ($4, $5)"), "{sql}");
        assert_eq!(
            RESOLVABLE_STATUSES,
            &[
                git_provider_account::STATUS_CONNECTED,
                git_provider_account::STATUS_DEGRADED
            ]
        );
    }

    #[test]
    fn normalize_host_url_vectors() {
        // FX-GIT-01 executed vectors.
        assert_eq!(
            normalize_host_url("https://github.com/"),
            "https://github.com"
        );
        assert_eq!(
            normalize_host_url("gitlab.example.com/"),
            "https://gitlab.example.com"
        );
        assert_eq!(
            normalize_host_url("http://git.internal:8080/"),
            "http://git.internal:8080"
        );
        assert_eq!(normalize_host_url(""), "");
        // Scheme check is case-sensitive (services.py:45).
        assert_eq!(
            normalize_host_url("HTTP://upper.example/"),
            "https://HTTP://upper.example"
        );
        assert_eq!(
            normalize_host_url("  gitlab.example.com  "),
            "https://gitlab.example.com"
        );
    }

    #[test]
    fn page_rule_vectors() {
        assert_eq!(parse_page_param(None), 1);
        assert_eq!(parse_page_param(Some("3")), 3);
        assert_eq!(parse_page_param(Some("0")), 1);
        assert_eq!(parse_page_param(Some("-2")), 1);
        assert_eq!(parse_page_param(Some("abc")), 1);
        assert_eq!(parse_page_param(Some(" 2 ")), 2);
        assert_eq!(parse_page_param(Some("2.5")), 1);
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
        "CREATE TEMPORARY TABLE git_provider_accounts (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, workspace_id UUID NOT NULL, provider VARCHAR(32) NOT NULL, host_url VARCHAR(500) NOT NULL, auth_type VARCHAR(32) NOT NULL, external_account_id VARCHAR(255) NOT NULL, external_account_login VARCHAR(255) NOT NULL, display_name VARCHAR(255) NOT NULL, capabilities JSONB NOT NULL, credential_config JSONB NOT NULL, workspace_integration_id UUID, status VARCHAR(16) NOT NULL, verified_at TIMESTAMPTZ, last_check_error TEXT NOT NULL, metadata JSONB NOT NULL)",
        "CREATE TEMPORARY TABLE git_repositories (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, provider VARCHAR(32) NOT NULL, host_url VARCHAR(500) NOT NULL, external_id VARCHAR(255) NOT NULL, namespace VARCHAR(500) NOT NULL, name VARCHAR(500) NOT NULL, full_name VARCHAR(1000) NOT NULL, web_url VARCHAR(1000) NOT NULL, clone_url_http VARCHAR(1000) NOT NULL, clone_url_ssh VARCHAR(1000) NOT NULL, default_branch VARCHAR(255) NOT NULL, is_private BOOLEAN NOT NULL, metadata JSONB NOT NULL)",
        "CREATE TEMPORARY TABLE git_repository_bindings (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, project_id UUID NOT NULL, workspace_id UUID NOT NULL, repository_id UUID NOT NULL, provider_account_id UUID NOT NULL, actor_id UUID NOT NULL, is_sync_enabled BOOLEAN NOT NULL, clone_auth_mode VARCHAR(32) NOT NULL, last_synced_at TIMESTAMPTZ, last_sync_error TEXT NOT NULL, metadata JSONB NOT NULL)",
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
        uuid::Uuid::parse_str(&format!("33333333-3333-3333-3333-3333333333{suffix:02}"))
            .expect("fixed test uuid")
    }

    fn live_ts(secs: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(secs, 0).expect("fixed test timestamp")
    }

    async fn seed_workspace(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: uuid::Uuid,
        slug: &str,
        deleted: Option<chrono::DateTime<chrono::Utc>>,
    ) {
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, name, slug) VALUES ($1, $2, $2, NULL, NULL, $3, 'n', $4)",
        )
        .bind(id)
        .bind(live_ts(1_700_000_000))
        .bind(deleted)
        .bind(slug)
        .execute(&mut **tx)
        .await
        .expect("seed workspace");
    }

    #[allow(clippy::too_many_arguments)]
    async fn seed_account(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: uuid::Uuid,
        ws: uuid::Uuid,
        provider: &str,
        host: &str,
        display: &str,
        status: &str,
        deleted: Option<chrono::DateTime<chrono::Utc>>,
    ) {
        sqlx::query(
            "INSERT INTO git_provider_accounts (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, provider, host_url, auth_type, external_account_id, external_account_login, display_name, capabilities, credential_config, workspace_integration_id, status, verified_at, last_check_error, metadata) VALUES ($1, $2, $2, NULL, NULL, $3, $4, $5, $6, 'pat', 'e', 'l', $7, '{}', '{}', NULL, $8, NULL, '', '{}')",
        )
        .bind(id)
        .bind(live_ts(1_700_000_000))
        .bind(deleted)
        .bind(ws)
        .bind(provider)
        .bind(host)
        .bind(display)
        .bind(status)
        .execute(&mut **tx)
        .await
        .expect("seed account");
    }

    /// FX-GIT-01 replay: ordering + workspace scoping + soft-delete.
    #[tokio::test]
    async fn live_account_list_and_detail() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let (wsa, wsb) = (live_uuid(1), live_uuid(2));
        seed_workspace(&mut tx, wsa, "acme", None).await;
        seed_workspace(&mut tx, wsb, "other", None).await;
        // Deliberately unordered inserts: gitlab/b, github/z, github/a.
        seed_account(
            &mut tx,
            live_uuid(11),
            wsa,
            "gitlab",
            "https://g.example",
            "b",
            "connected",
            None,
        )
        .await;
        seed_account(
            &mut tx,
            live_uuid(12),
            wsa,
            "github",
            "https://github.com",
            "z",
            "connected",
            None,
        )
        .await;
        seed_account(
            &mut tx,
            live_uuid(13),
            wsa,
            "github",
            "https://github.com",
            "a",
            "connected",
            None,
        )
        .await;
        seed_account(
            &mut tx,
            live_uuid(14),
            wsa,
            "github",
            "https://github.com",
            "aaa",
            "connected",
            Some(live_ts(1_700_000_100)),
        )
        .await;
        seed_account(
            &mut tx,
            live_uuid(15),
            wsb,
            "github",
            "https://github.com",
            "solo",
            "connected",
            None,
        )
        .await;

        let got = fetch_provider_account_list(&mut *tx, wsa)
            .await
            .expect("list");
        // provider ASC ("github" < "gitlab" on the second byte), then
        // host_url, then display_name.
        assert_eq!(
            got.iter()
                .map(|a| a.display_name.clone())
                .collect::<Vec<_>>(),
            vec!["a".to_string(), "z".to_string(), "b".to_string()]
        );
        // Detail scoping: right id + right slug hits; cross-workspace misses.
        assert_eq!(
            fetch_provider_account_detail(&mut *tx, live_uuid(12), "acme")
                .await
                .expect("detail")
                .map(|a| a.display_name),
            Some("z".to_string())
        );
        assert_eq!(
            fetch_provider_account_detail(&mut *tx, live_uuid(12), "other")
                .await
                .expect("detail"),
            None
        );
        assert_eq!(
            fetch_provider_account_detail(&mut *tx, live_uuid(14), "acme")
                .await
                .expect("detail"),
            None
        );
        tx.rollback().await.expect("rollback");
    }

    /// FX-GIT-01 resolution replay: host + live-status filter.
    #[tokio::test]
    async fn live_account_resolution() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let ws = live_uuid(1);
        seed_workspace(&mut tx, ws, "acme", None).await;
        seed_account(
            &mut tx,
            live_uuid(21),
            ws,
            "github",
            "https://github.com",
            "ok",
            "connected",
            None,
        )
        .await;
        seed_account(
            &mut tx,
            live_uuid(22),
            ws,
            "github",
            "https://github.com",
            "dg",
            "degraded",
            None,
        )
        .await;
        seed_account(
            &mut tx,
            live_uuid(23),
            ws,
            "github",
            "https://github.com",
            "rv",
            "revoked",
            None,
        )
        .await;
        seed_account(
            &mut tx,
            live_uuid(24),
            ws,
            "gitlab",
            "https://g.example",
            "gl",
            "connected",
            None,
        )
        .await;

        let got = fetch_provider_account_resolution(&mut *tx, ws, "github", "https://github.com")
            .await
            .expect("resolution");
        assert_eq!(
            got.iter()
                .map(|a| a.display_name.clone())
                .collect::<Vec<_>>(),
            vec!["ok".to_string(), "dg".to_string()]
        );
        tx.rollback().await.expect("rollback");
    }

    /// FX-GIT-02 replay: binding read with joined repository + account.
    #[tokio::test]
    async fn live_binding_for_project() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let (ws, proj, repo, acct, bind) = (
            live_uuid(1),
            live_uuid(2),
            live_uuid(3),
            live_uuid(4),
            live_uuid(5),
        );
        seed_workspace(&mut tx, ws, "acme", None).await;
        seed_account(
            &mut tx,
            acct,
            ws,
            "github",
            "https://github.com",
            "a",
            "connected",
            None,
        )
        .await;
        sqlx::query(
            "INSERT INTO git_repositories (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, provider, host_url, external_id, namespace, name, full_name, web_url, clone_url_http, clone_url_ssh, default_branch, is_private, metadata) VALUES ($1, $2, $2, NULL, NULL, NULL, 'github', 'https://github.com', '99', 'owner', 'repo', 'owner/repo', 'https://github.com/owner/repo', '', '', 'main', FALSE, '{}')",
        )
        .bind(repo)
        .bind(live_ts(1_700_000_000))
        .execute(&mut *tx)
        .await
        .expect("seed repo");
        sqlx::query(
            "INSERT INTO git_repository_bindings (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, repository_id, provider_account_id, actor_id, is_sync_enabled, clone_auth_mode, last_synced_at, last_sync_error, metadata) VALUES ($1, $2, $2, NULL, NULL, NULL, $3, $4, $5, $6, $4, TRUE, 'public', NULL, '', '{}')",
        )
        .bind(bind)
        .bind(live_ts(1_700_000_000))
        .bind(proj)
        .bind(ws)
        .bind(repo)
        .bind(acct)
        .execute(&mut *tx)
        .await
        .expect("seed binding");

        let got = fetch_binding_for_project(&mut *tx, proj, "acme")
            .await
            .expect("binding")
            .expect("a row");
        assert_eq!(got.binding.id, bind);
        assert!(got.binding.is_sync_enabled);
        assert_eq!(got.repository.full_name, "owner/repo");
        assert_eq!(got.repository.external_id, "99");
        assert!(!got.repository.is_private);
        assert_eq!(got.provider_account.id, acct);
        assert_eq!(got.provider_account.status, "connected");
        // Unknown project reads as absent (the {"bound": False} branch).
        assert_eq!(
            fetch_binding_for_project(&mut *tx, live_uuid(9), "acme")
                .await
                .expect("miss"),
            None
        );
        tx.rollback().await.expect("rollback");
    }
}
