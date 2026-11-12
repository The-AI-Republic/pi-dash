//! Git-integration query reads (D-33, stage 5).
//!
//! Ports the database read shapes behind
//! `apps/api/pi_dash/app/views/integration/git.py:50-175` and the
//! `get_binding` / `_provider_account_queryset` helpers in
//! `apps/api/pi_dash/integrations/git/services.py:63-70,353-358`.
//! Fixtures FX-GIT-01, FX-GIT-02
//! (`rust-api/fixtures/app_integrations/fx-git-0{1,2}-*.json`).
//!
//! Every executor is a SeaORM [`ConnectionTrait`](sea_orm::ConnectionTrait)
//! (`sea_orm_connection` on the pool, `TxConn` inside a transaction);
//! row structs reuse the D-05 models (`crate::integrations::git_models`,
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

use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, FromQueryResult, JoinType, QueryFilter, QueryOrder,
    QuerySelect, RelationTrait, Select,
};

use crate::entity::{git_provider_accounts, git_repositories, git_repository_bindings, workspaces};
use crate::integrations::git_models::{
    git_provider_account::{self, GitProviderAccount},
    git_repository_binding::GitRepositoryBinding,
};

/// Live account statuses admitted by `_provider_account_queryset`
/// (`services.py:63-70`).
pub const RESOLVABLE_STATUSES: &[&str] = &[
    git_provider_account::STATUS_CONNECTED,
    git_provider_account::STATUS_DEGRADED,
];

// ---------------------------------------------------------------------------
// Provider-account list (git.py:57-67)
// ---------------------------------------------------------------------------

/// `GitProviderAccount.objects.filter(workspace=workspace).order_by(
/// "provider", "host_url", "display_name")` (`git.py:61-63`).
pub fn provider_account_list_query(
    workspace_id: uuid::Uuid,
) -> Select<git_provider_accounts::Entity> {
    git_provider_accounts::Entity::find()
        .filter(git_provider_accounts::Column::DeletedAt.is_null())
        .filter(git_provider_accounts::Column::WorkspaceId.eq(workspace_id))
        .order_by_asc(git_provider_accounts::Column::Provider)
        .order_by_asc(git_provider_accounts::Column::HostUrl)
        .order_by_asc(git_provider_accounts::Column::DisplayName)
}

/// Ordered provider accounts for one workspace
/// (`git.py:64-67`, `{"accounts": [...]}`).
pub async fn fetch_provider_account_list(
    db: &impl ConnectionTrait,
    workspace_id: uuid::Uuid,
) -> Result<Vec<GitProviderAccount>, sea_orm::DbErr> {
    let rows = provider_account_list_query(workspace_id).all(db).await?;
    Ok(rows.into_iter().map(GitProviderAccount::from).collect())
}

// ---------------------------------------------------------------------------
// Provider-account detail (git.py:97-101, delete path :103-114 reads :105)
// ---------------------------------------------------------------------------

/// `get_object_or_404(GitProviderAccount, id=account_id,
/// workspace__slug=slug)` (`git.py:100,105`): the `workspace__slug`
/// traversal inner-joins `workspaces` (non-nullable FK) and applies the
/// default manager's `deleted_at IS NULL` on both tables.
pub fn provider_account_detail_query(
    account_id: uuid::Uuid,
    workspace_slug: &str,
) -> Select<git_provider_accounts::Entity> {
    git_provider_accounts::Entity::find()
        .join(
            JoinType::InnerJoin,
            git_provider_accounts::Relation::Workspaces.def(),
        )
        .filter(git_provider_accounts::Column::DeletedAt.is_null())
        .filter(git_provider_accounts::Column::Id.eq(account_id))
        .filter(workspaces::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(workspace_slug.to_owned()))
        .limit(1)
}
/// One provider account by id scoped to a workspace slug, or `None`
/// (the view's 404 branch).
pub async fn fetch_provider_account_detail(
    db: &impl ConnectionTrait,
    account_id: uuid::Uuid,
    workspace_slug: &str,
) -> Result<Option<GitProviderAccount>, sea_orm::DbErr> {
    let row = provider_account_detail_query(account_id, workspace_slug)
        .one(db)
        .await?;
    Ok(row.map(GitProviderAccount::from))
}

// ---------------------------------------------------------------------------
// Project binding read (services.py:353-358, git.py:132-138)
// ---------------------------------------------------------------------------

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

impl From<git_repositories::Model> for BoundRepository {
    fn from(row: git_repositories::Model) -> Self {
        Self {
            id: row.id,
            provider: row.provider,
            host_url: row.host_url,
            external_id: row.external_id,
            namespace: row.namespace,
            name: row.name,
            full_name: row.full_name,
            web_url: row.web_url,
            clone_url_http: row.clone_url_http,
            clone_url_ssh: row.clone_url_ssh,
            default_branch: row.default_branch,
            is_private: row.is_private,
        }
    }
}

/// Account fields read through a project binding (the `degraded` inputs).
#[derive(Debug, Clone, PartialEq)]
pub struct BoundAccount {
    pub id: uuid::Uuid,
    pub status: String,
    pub last_check_error: String,
}

impl From<git_provider_accounts::Model> for BoundAccount {
    fn from(row: git_provider_accounts::Model) -> Self {
        Self {
            id: row.id,
            status: row.status,
            last_check_error: row.last_check_error,
        }
    }
}

/// One project binding with its `select_related` repository and provider
/// account (`services.py:353-358`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectBinding {
    pub binding: GitRepositoryBinding,
    pub repository: BoundRepository,
    pub provider_account: BoundAccount,
}

/// One `binding_for_project_query` row. Joined columns carry the
/// `<table>__<col>` alias prefix so every output name is unique by
/// construction; decode is by name.
#[derive(Debug, Clone, FromQueryResult)]
#[allow(non_snake_case)]
pub struct BindingRow {
    pub id: uuid::Uuid,
    pub created_at: sea_orm::prelude::DateTimeWithTimeZone,
    pub updated_at: sea_orm::prelude::DateTimeWithTimeZone,
    pub created_by_id: Option<uuid::Uuid>,
    pub updated_by_id: Option<uuid::Uuid>,
    pub deleted_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    pub project_id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub repository_id: uuid::Uuid,
    pub provider_account_id: uuid::Uuid,
    pub actor_id: uuid::Uuid,
    pub is_sync_enabled: bool,
    pub clone_auth_mode: String,
    pub last_synced_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    pub last_sync_error: String,
    pub metadata: serde_json::Value,
    pub git_repositories__id: uuid::Uuid,
    pub git_repositories__provider: String,
    pub git_repositories__host_url: String,
    pub git_repositories__external_id: String,
    pub git_repositories__namespace: String,
    pub git_repositories__name: String,
    pub git_repositories__full_name: String,
    pub git_repositories__web_url: String,
    pub git_repositories__clone_url_http: String,
    pub git_repositories__clone_url_ssh: String,
    pub git_repositories__default_branch: String,
    pub git_repositories__is_private: bool,
    pub git_provider_accounts__id: uuid::Uuid,
    pub git_provider_accounts__status: String,
    pub git_provider_accounts__last_check_error: String,
}

impl From<BindingRow> for ProjectBinding {
    fn from(row: BindingRow) -> Self {
        let at = |at: sea_orm::prelude::DateTimeWithTimeZone| at.with_timezone(&chrono::Utc);
        Self {
            binding: GitRepositoryBinding {
                id: row.id,
                created_at: at(row.created_at),
                updated_at: at(row.updated_at),
                created_by_id: row.created_by_id,
                updated_by_id: row.updated_by_id,
                deleted_at: row.deleted_at.map(at),
                project_id: row.project_id,
                workspace_id: row.workspace_id,
                repository_id: row.repository_id,
                provider_account_id: row.provider_account_id,
                actor_id: row.actor_id,
                is_sync_enabled: row.is_sync_enabled,
                clone_auth_mode: row.clone_auth_mode,
                last_synced_at: row.last_synced_at.map(at),
                last_sync_error: row.last_sync_error,
                metadata: row.metadata,
            },
            repository: BoundRepository {
                id: row.git_repositories__id,
                provider: row.git_repositories__provider,
                host_url: row.git_repositories__host_url,
                external_id: row.git_repositories__external_id,
                namespace: row.git_repositories__namespace,
                name: row.git_repositories__name,
                full_name: row.git_repositories__full_name,
                web_url: row.git_repositories__web_url,
                clone_url_http: row.git_repositories__clone_url_http,
                clone_url_ssh: row.git_repositories__clone_url_ssh,
                default_branch: row.git_repositories__default_branch,
                is_private: row.git_repositories__is_private,
            },
            provider_account: BoundAccount {
                id: row.git_provider_accounts__id,
                status: row.git_provider_accounts__status,
                last_check_error: row.git_provider_accounts__last_check_error,
            },
        }
    }
}

/// `GitRepositoryBinding.objects.filter(project_id=project_id,
/// workspace__slug=slug).select_related("repository",
/// "provider_account").first()` (`services.py:353-358`).
pub fn binding_for_project_query(
    project_id: uuid::Uuid,
    workspace_slug: &str,
) -> Select<git_repository_bindings::Entity> {
    git_repository_bindings::Entity::find()
        .select_only()
        .columns([
            git_repository_bindings::Column::Id,
            git_repository_bindings::Column::CreatedAt,
            git_repository_bindings::Column::UpdatedAt,
            git_repository_bindings::Column::CreatedById,
            git_repository_bindings::Column::UpdatedById,
            git_repository_bindings::Column::DeletedAt,
            git_repository_bindings::Column::ProjectId,
            git_repository_bindings::Column::WorkspaceId,
            git_repository_bindings::Column::RepositoryId,
            git_repository_bindings::Column::ProviderAccountId,
            git_repository_bindings::Column::ActorId,
            git_repository_bindings::Column::IsSyncEnabled,
            git_repository_bindings::Column::CloneAuthMode,
            git_repository_bindings::Column::LastSyncedAt,
            git_repository_bindings::Column::LastSyncError,
            git_repository_bindings::Column::Metadata,
        ])
        .column_as(git_repositories::Column::Id, "git_repositories__id")
        .column_as(
            git_repositories::Column::Provider,
            "git_repositories__provider",
        )
        .column_as(
            git_repositories::Column::HostUrl,
            "git_repositories__host_url",
        )
        .column_as(
            git_repositories::Column::ExternalId,
            "git_repositories__external_id",
        )
        .column_as(
            git_repositories::Column::Namespace,
            "git_repositories__namespace",
        )
        .column_as(git_repositories::Column::Name, "git_repositories__name")
        .column_as(
            git_repositories::Column::FullName,
            "git_repositories__full_name",
        )
        .column_as(
            git_repositories::Column::WebUrl,
            "git_repositories__web_url",
        )
        .column_as(
            git_repositories::Column::CloneUrlHttp,
            "git_repositories__clone_url_http",
        )
        .column_as(
            git_repositories::Column::CloneUrlSsh,
            "git_repositories__clone_url_ssh",
        )
        .column_as(
            git_repositories::Column::DefaultBranch,
            "git_repositories__default_branch",
        )
        .column_as(
            git_repositories::Column::IsPrivate,
            "git_repositories__is_private",
        )
        .column_as(
            git_provider_accounts::Column::Id,
            "git_provider_accounts__id",
        )
        .column_as(
            git_provider_accounts::Column::Status,
            "git_provider_accounts__status",
        )
        .column_as(
            git_provider_accounts::Column::LastCheckError,
            "git_provider_accounts__last_check_error",
        )
        .join(
            JoinType::InnerJoin,
            git_repository_bindings::Relation::Workspaces.def(),
        )
        .join(
            JoinType::InnerJoin,
            git_repository_bindings::Relation::GitRepositories.def(),
        )
        .join(
            JoinType::InnerJoin,
            git_repository_bindings::Relation::GitProviderAccounts.def(),
        )
        .filter(git_repository_bindings::Column::DeletedAt.is_null())
        .filter(git_repository_bindings::Column::ProjectId.eq(project_id))
        .filter(workspaces::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(workspace_slug.to_owned()))
        .filter(git_repositories::Column::DeletedAt.is_null())
        .filter(git_provider_accounts::Column::DeletedAt.is_null())
        .limit(1)
}

/// The project's binding (`None` renders `{"bound": False}`,
/// `git.py:136-137`).
pub async fn fetch_binding_for_project(
    db: &impl ConnectionTrait,
    project_id: uuid::Uuid,
    workspace_slug: &str,
) -> Result<Option<ProjectBinding>, sea_orm::DbErr> {
    let row = binding_for_project_query(project_id, workspace_slug)
        .into_model::<BindingRow>()
        .one(db)
        .await?;
    Ok(row.map(ProjectBinding::from))
}

// ---------------------------------------------------------------------------
// Bind-resolution queryset (services.py:63-70,110-131)
// ---------------------------------------------------------------------------

/// `_provider_account_queryset(workspace, provider, host_url)`
/// (`services.py:63-70`): live accounts for one host. The caller passes
/// `host_url` already through [`normalize_host_url`].
pub fn provider_account_resolution_query(
    workspace_id: uuid::Uuid,
    provider: &str,
    host_url: &str,
) -> Select<git_provider_accounts::Entity> {
    git_provider_accounts::Entity::find()
        .filter(git_provider_accounts::Column::DeletedAt.is_null())
        .filter(git_provider_accounts::Column::WorkspaceId.eq(workspace_id))
        .filter(git_provider_accounts::Column::Provider.eq(provider.to_owned()))
        .filter(git_provider_accounts::Column::HostUrl.eq(host_url.to_owned()))
        .filter(git_provider_accounts::Column::Status.is_in([
            git_provider_account::STATUS_CONNECTED,
            git_provider_account::STATUS_DEGRADED,
        ]))
        .order_by_asc(git_provider_accounts::Column::Id)
}

/// Candidate accounts for binding a repository host: explicit
/// `provider_account_id` narrows to one id (404 when absent from this
/// set); no id with 0 rows raises 409 Required and >1 rows raises 409
/// Ambiguous (`services.py:110-131`). The count/branching itself is
/// handler logic; this is the shared read.
pub async fn fetch_provider_account_resolution(
    db: &impl ConnectionTrait,
    workspace_id: uuid::Uuid,
    provider: &str,
    host_url: &str,
) -> Result<Vec<GitProviderAccount>, sea_orm::DbErr> {
    let rows = provider_account_resolution_query(workspace_id, provider, host_url)
        .all(db)
        .await?;
    Ok(rows.into_iter().map(GitProviderAccount::from).collect())
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
    use sea_orm::{DbBackend, QueryTrait};

    fn shape(query: Select<git_provider_accounts::Entity>) -> String {
        query.build(DbBackend::Postgres).to_string()
    }

    #[test]
    fn list_orders_by_provider_host_display_name() {
        let ws = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid");
        let sql = shape(provider_account_list_query(ws));
        assert!(sql.contains(r#"FROM "git_provider_accounts""#), "{sql}");
        assert!(sql.contains(r#""deleted_at" IS NULL"#), "{sql}");
        assert!(
            sql.contains(&format!(r#""workspace_id" = '{ws}'"#)),
            "{sql}"
        );
        let order = sql.find("ORDER BY").expect("order");
        let tail = &sql[order..];
        assert!(tail.contains(r#""provider" ASC"#), "{tail}");
        assert!(tail.contains(r#""host_url" ASC"#), "{tail}");
        assert!(tail.contains(r#""display_name" ASC"#), "{tail}");
    }

    #[test]
    fn detail_scopes_id_to_workspace_slug() {
        let id = uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("uuid");
        let sql = shape(provider_account_detail_query(id, "acme"));
        assert!(sql.contains(r#"FROM "git_provider_accounts""#), "{sql}");
        assert!(sql.contains(r#"INNER JOIN "workspaces""#), "{sql}");
        assert!(sql.contains(&format!(r#""id" = '{id}'"#)), "{sql}");
        assert!(sql.contains(r#""slug" = 'acme'"#), "{sql}");
        assert!(sql.contains("LIMIT 1"), "{sql}");
        assert_eq!(
            sql.match_indices("deleted_at\" IS NULL").count(),
            2,
            "{sql}"
        );
    }

    #[test]
    fn binding_joins_repo_account_and_workspace() {
        let proj = uuid::Uuid::parse_str("33333333-3333-3333-3333-333333333333").expect("uuid");
        let sql = binding_for_project_query(proj, "acme")
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains(r#"FROM "git_repository_bindings""#), "{sql}");
        assert!(sql.contains(r#"INNER JOIN "git_repositories""#), "{sql}");
        assert!(
            sql.contains(r#"INNER JOIN "git_provider_accounts""#),
            "{sql}"
        );
        assert!(
            sql.contains(&format!(r#""project_id" = '{proj}'"#)),
            "{sql}"
        );
        assert!(sql.contains(r#""workspaces"."slug" = 'acme'"#), "{sql}");
        assert!(sql.contains(r#""git_repositories__full_name""#), "{sql}");
        assert!(sql.contains(r#""git_provider_accounts__status""#), "{sql}");
        assert!(sql.contains("LIMIT 1"), "{sql}");
    }

    #[test]
    fn resolution_filters_host_and_live_statuses() {
        let ws = uuid::Uuid::parse_str("44444444-4444-4444-4444-444444444444").expect("uuid");
        let sql = shape(provider_account_resolution_query(
            ws,
            "github",
            "https://github.com",
        ));
        assert!(sql.contains(r#""provider" = 'github'"#), "{sql}");
        assert!(
            sql.contains(r#""host_url" = 'https://github.com'"#),
            "{sql}"
        );
        assert!(sql.contains("IN"), "{sql}");
        assert!(sql.contains("'connected'"), "{sql}");
        assert!(sql.contains("'degraded'"), "{sql}");
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

        let conn = crate::orm_tx::TxConn::new(&mut tx);
        let got = fetch_provider_account_list(&conn, wsa).await.expect("list");
        assert_eq!(
            got.iter()
                .map(|a| a.display_name.clone())
                .collect::<Vec<_>>(),
            vec!["a".to_string(), "z".to_string(), "b".to_string()]
        );
        assert_eq!(
            fetch_provider_account_detail(&conn, live_uuid(12), "acme")
                .await
                .expect("detail")
                .map(|a| a.display_name),
            Some("z".to_string())
        );
        assert_eq!(
            fetch_provider_account_detail(&conn, live_uuid(12), "other")
                .await
                .expect("detail"),
            None
        );
        assert_eq!(
            fetch_provider_account_detail(&conn, live_uuid(14), "acme")
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

        let conn = crate::orm_tx::TxConn::new(&mut tx);
        let got = fetch_provider_account_resolution(&conn, ws, "github", "https://github.com")
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

        let conn = crate::orm_tx::TxConn::new(&mut tx);
        let got = fetch_binding_for_project(&conn, proj, "acme")
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
        assert_eq!(
            fetch_binding_for_project(&conn, live_uuid(9), "acme")
                .await
                .expect("miss"),
            None
        );
        tx.rollback().await.expect("rollback");
    }
}
