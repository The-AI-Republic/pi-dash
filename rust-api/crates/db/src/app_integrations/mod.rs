//! App integrations/webhooks table models (D-33, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/webhook.py:34-89` (`Webhook`,
//! `WebhookLog`, `ProjectWebhook`) and
//! `apps/api/pi_dash/db/models/integration/base.py:16-60` (`Integration`,
//! `WorkspaceIntegration`) for the db layer, adopting the Django-owned
//! schema column-for-column. Migrations are not ported; Django stays schema
//! owner until switchover.
//!
//! * [`models_webhook`] — the 5 tables (PIDASHCONV-380). `ProjectWebhook`
//!   has no owning sub-issue (it is absent from FX-MDL-01's 15 models and
//!   the cancelled siblings 390/403/418 do not cover it); it is ported here
//!   as the topological closure of `webhook.py`.
//!
//! The layout mirrors the D-05 models convention (PIDASHCONV-144,
//! `crate::integrations`): one `pub mod` per table with `TABLE`,
//! `ORDERING`, and `COLUMNS` consts in Django `_meta` field order, a row
//! struct, enum ports with stored-value round-trips, and FK / unique /
//! index consts. `#[cfg(test)]` asserts columns field-for-field against
//! `rust-api/fixtures/app_integrations/fx-mdl-01-model-columns.json`
//! (PIDASHCONV-337).
//!
//! # Column order and the fixture shape
//!
//! `COLUMNS` holds the audit prefix (`id`, `created_at`, `updated_at`,
//! `created_by_id`, `updated_by_id`, `deleted_at`; see [`AUDIT_COLUMNS`]),
//! then `project_id`, `workspace_id` for `ProjectBaseModel` tables (see
//! [`PROJECT_COLUMNS`]; `workspace` is auto-set from `project` on save,
//! `db/models/project.py:302-311`), then the declared fields in source
//! order with Django attnames (`workspace` -> `workspace_id`, ...). The
//! FX-MDL-01 fixture lists descriptive entries (including `/`-grouped
//! columns and `+ audit base` markers), so [`test_support`] expands them
//! to attnames before comparing. Order is cosmetic for query building;
//! membership is the contract.
//!
//! # Wiring note
//!
//! The crate root declares `pub mod app_integrations;` (one-line wiring in
//! the port PR, following the PIDASHCONV-284 precedent for `app_intake`).
//! Later D-33 layer issues add siblings here (`queries_webhook.rs` for
//! PIDASHCONV-428); the services-layer serializers (PIDASHCONV-365) live
//! under `crates/services/src/app_integrations/` and do not touch this
//! module.

//! App-integration query reads (D-33, stage 5).
//!
//! Ports the database read shapes behind
//! `apps/api/pi_dash/app/views/integration/git.py` and
//! `apps/api/pi_dash/app/views/integration/github.py`, plus the
//! pure read-helper shapes in
//! `apps/api/pi_dash/utils/github_app_auth.py` (no live calls).
//!
//! * [`queries_git`] — provider-account list/detail, the
//!   `get_binding` project lookup, and the bind-resolution queryset
//!   (`services.py:63-70`), with the `page` rule and `normalize_host_url`
//!   pure helpers. Fixtures FX-GIT-01, FX-GIT-02.
//! * [`queries_github`] — workspace-integration lookup, status/repos
//!   reads, the ADMIN membership scoping (`github.py:628-632`), project
//!   sync/binding status reads, the install-session callback read, and
//!   the app-auth pure shapes (JWT claims, headers, exchange payload,
//!   installations URL + `Link` paging). Fixtures FX-GHA-01..03.
//!
//! Same-SQL-semantics notes (translate, don't redesign):
//!
//! * Every read carries the default-manager `deleted_at IS NULL`
//!   conjunct (`db/mixins.py:57-58`); the one explicit
//!   `deleted_at__isnull=True` in this domain's paths doubles it, as in
//!   the merged `loop/queries` precedent.
//! * `get_object_or_404` is `LIMIT 1` here: the filtered columns are
//!   unique (`slug` UNIQUE, PK `id`, `state` UNIQUE), so Django's
//!   `get()` `LIMIT 21` and `LIMIT 1` return the same row.
//! * `select_related` on non-nullable FKs is `INNER JOIN`; joined-table
//!   columns are aliased `<table>__<col>` because Django maps its
//!   duplicate column names positionally while `sqlx` maps by name —
//!   same rows, same joins, same predicates.
//!
//! Extension note: PIDASHCONV-380 adds `models_webhook.rs` and
//! PIDASHCONV-428 adds `queries_webhook.rs` to this directory; keep
//! every `pub mod` line when rebasing (merge rule b).

pub mod models_webhook;

pub use models_webhook::{
    integration::Integration, project_webhook::ProjectWebhook, webhook::Webhook,
    webhook_log::WebhookLog, workspace_integration::WorkspaceIntegration, NetworkType, OnDelete,
};

/// Audit prefix shared by every model (`BaseModel` + `TimeAuditModel` +
/// `UserAuditModel` + `SoftDeleteModel`; matches the D-05
/// `integrations::AUDIT_COLUMNS` order).
pub const AUDIT_COLUMNS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
];

/// `ProjectBaseModel` prefix (`db/models/project.py:302-311`), inserted
/// between [`AUDIT_COLUMNS`] and the declared fields.
pub const PROJECT_COLUMNS: &[&str] = &["project_id", "workspace_id"];

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    /// Expand one FX-MDL-01 fixture `columns` entry into Django attnames.
    /// Entries are descriptive (`"workspace FK CASCADE related ..."`,
    /// `"project/issue/module/cycle/issue_comment/is_internal Bool False"`,
    /// `"+ audit base"`); `/`-separated heads expand to one attname each,
    /// and ` FK ` entries gain the `_id` suffix.
    pub(crate) fn fixture_attnames(entry: &str) -> Vec<String> {
        if entry == "+ audit base" {
            return super::AUDIT_COLUMNS
                .iter()
                .map(|c| (*c).to_string())
                .collect();
        }
        if let Some(group) = entry.strip_prefix("+ ") {
            return group
                .split('/')
                .map(|part| {
                    let part = part.trim();
                    if part == "created_by" || part == "updated_by" {
                        format!("{part}_id")
                    } else {
                        part.to_string()
                    }
                })
                .collect();
        }
        let head = entry.split([' ', '(']).next().unwrap_or_default();
        // `webhook UUIDField (NOT a FK ...)` names a plain UUID column, not
        // a foreign key, so it must not gain the `_id` suffix.
        let is_fk = entry.contains(" FK ") && !entry.contains("NOT a FK");
        head.split('/')
            .filter(|part| !part.is_empty())
            .map(|part| {
                if is_fk {
                    format!("{part}_id")
                } else {
                    part.to_string()
                }
            })
            .collect()
    }

    pub(crate) fn fixtures_path() -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/app_integrations/fx-mdl-01-model-columns.json")
    }

    pub(crate) fn fixture_model(name: &str) -> serde_json::Value {
        let body = std::fs::read_to_string(fixtures_path())
            .unwrap_or_else(|e| panic!("read fx-mdl-01-model-columns.json: {e}"));
        let v: serde_json::Value =
            serde_json::from_str(&body).expect("fx-mdl-01-model-columns.json is valid JSON");
        let m = v["models"][name].clone();
        assert!(m.is_object(), "fixture has model {name}");
        m
    }

    /// Declared-field attnames from the fixture, in fixture order.
    pub(crate) fn fixture_columns(name: &str) -> Vec<String> {
        fixture_model(name)["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("fixture {name} has columns array"))
            .iter()
            .flat_map(|c| {
                fixture_attnames(
                    c.as_str()
                        .unwrap_or_else(|| panic!("fixture {name} column is a string")),
                )
            })
            .collect()
    }

    pub(crate) fn fixture_table(name: &str) -> String {
        fixture_model(name)["table"].as_str().unwrap().to_string()
    }

    /// Copy a column const into an owned vec so assertions compare two
    /// runtime values.
    pub(crate) fn owned_columns(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }
}

pub mod queries_git;
pub mod queries_github;

/// `db.Workspace` table (`db/models/workspace.py:181`).
pub const WORKSPACE_TABLE: &str = "workspaces";
/// `db.WorkspaceMember` table (`db/models/workspace.py:226`).
pub const WORKSPACE_MEMBER_TABLE: &str = "workspace_members";
/// `db.Integration` table (`db/models/integration/base.py:36`).
pub const INTEGRATION_TABLE: &str = "integrations";
/// `db.WorkspaceIntegration` table (`db/models/integration/base.py:59`).
pub const WORKSPACE_INTEGRATION_TABLE: &str = "workspace_integrations";
/// The `Integration.provider` value this domain reads
/// (`github.py:91-99`, `_get_or_create_github_integration`).
pub const GITHUB_PROVIDER: &str = "github";
/// `ROLE.ADMIN.value` (`app/permissions/base.py:14`); `WorkspaceMember.role`
/// is a `PositiveSmallIntegerField`, so the predicate compares integers.
pub const ROLE_ADMIN: i16 = 20;

/// `get_object_or_404(Workspace, slug=slug)` read half
/// (`git.py:53,60,71`; `github.py:448,507,551,576,997`): the live row's id.
/// Binds `$1 = slug`.
pub fn workspace_by_slug_sql() -> String {
    use sea_query::PostgresQueryBuilder;
    use sea_query::{Alias, Condition, Expr, Order, Query};
    let mut sel = Query::select();
    sel.from(Alias::new(WORKSPACE_TABLE.to_owned()));
    sel.column((Alias::new(WORKSPACE_TABLE), Alias::new("id")));
    sel.cond_where(
        Condition::all()
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("slug"))).eq(Expr::cust("$1"))),
    )
    .order_by((Alias::new(WORKSPACE_TABLE), Alias::new("id")), Order::Asc)
    .limit(1);
    sel.to_string(PostgresQueryBuilder)
}

/// Live workspace id for a slug, or `None` (the view's 404 branch).
pub async fn fetch_workspace_id_by_slug<'e, E>(
    ex: E,
    slug: &str,
) -> Result<Option<uuid::Uuid>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&workspace_by_slug_sql())
        .bind(slug)
        .fetch_optional(ex)
        .await?;
    row.map(|r| {
        use sqlx::Row;
        r.try_get("id")
    })
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_by_slug_shape() {
        assert_eq!(
            workspace_by_slug_sql(),
            r#"SELECT "workspaces"."id" FROM "workspaces" WHERE "workspaces"."deleted_at" IS NULL AND "workspaces"."slug" = ($1) ORDER BY "workspaces"."id" ASC LIMIT 1"#
        );
    }

    // -- live scratch-DB tests (env-gated) -------------------------------

    /// Scratch Postgres for the Done-when verification. Unset (plain
    /// `cargo test`) skips these; CI sets no database either, so the suite
    /// stays green offline. Run with e.g.
    /// `export DATABASE_URL=postgresql://user@/pidash_433_scratch?host=/tmp`
    /// for the real check. Tables are `TEMPORARY` and every test rolls
    /// back, so no scratch state escapes.
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
        uuid::Uuid::parse_str(&format!("11111111-1111-1111-1111-1111111111{suffix:02}"))
            .expect("fixed test uuid")
    }

    fn live_ts(secs: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(secs, 0).expect("fixed test timestamp")
    }

    #[tokio::test]
    async fn live_workspace_by_slug() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let ts = live_ts(1_700_000_000);
        for (id, slug, deleted) in [
            (live_uuid(1), "acme", None),
            (live_uuid(2), "gone", Some(ts)),
        ] {
            sqlx::query(
                "INSERT INTO workspaces (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, name, slug) VALUES ($1, $2, $2, NULL, NULL, $3, 'n', $4)",
            )
            .bind(id)
            .bind(ts)
            .bind(deleted)
            .bind(slug)
            .execute(&mut *tx)
            .await
            .expect("seed workspace");
        }
        assert_eq!(
            fetch_workspace_id_by_slug(&mut *tx, "acme")
                .await
                .expect("hit"),
            Some(live_uuid(1))
        );
        // Soft-deleted slug reads as absent (the view's 404 branch).
        assert_eq!(
            fetch_workspace_id_by_slug(&mut *tx, "gone")
                .await
                .expect("hit"),
            None
        );
        assert_eq!(
            fetch_workspace_id_by_slug(&mut *tx, "missing")
                .await
                .expect("hit"),
            None
        );
        tx.rollback().await.expect("rollback");
    }
}
