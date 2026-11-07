//! Webhook CRUD + secret-regenerate + log query units (D-33, stage 5).
//!
//! Ports the query paths in `apps/api/pi_dash/app/views/webhook/base.py`:
//! webhook list/detail/patch/delete (`WebhookEndpoint`, `:20-108`),
//! secret regenerate (`WebhookSecretRegenerateEndpoint`, `:111-118`), and
//! log list (`WebhookLogsEndpoint`, `:121-126`). Fixtures FX-WEB-01,
//! FX-WEB-02, FX-WEB-03
//! (`rust-api/fixtures/app_integrations/fx-web-0[123]-*.json`, PIDASHCONV-337).
//!
//! Same conventions as [`super::queries_git`]: SeaORM entities (`ConnectionTrait`), row structs reused from
//! [`super::models_webhook`]. The workspace-slug lookup is the shared
//! [`super::workspace_by_slug_sql`] / [`super::fetch_workspace_id_by_slug`]
//! (the POST `Workspace.objects.get(slug=slug)`, `base.py:23`); no
//! duplicate helper lives here.
//!
//! SQL semantics are Django's, quirks included (translate, don't redesign):
//!
//! * Every read carries the default-manager `deleted_at IS NULL` conjunct
//!   (`db/mixins.py:57-58`), including the joined `workspaces` row on
//!   `workspace__slug` traversals (domain convention, see the module docs
//!   on [`super`]).
//! * `filter()` keeps `Meta.ordering = ("-created_at",)`; `get()` clears it
//!   and caps rows (`django/db/models/query.py: QuerySet.get`,
//!   `MAX_GET_RESULTS = 21`). The `get()` predicates here are unique
//!   (PK `id`, UNIQUE `slug`), so the domain `LIMIT 1` convention returns
//!   the same row.
//! * `create()` (`serializers/webhook.py:55`) and every `save()` — patch
//!   (`serializers/webhook.py:90`, `super().update()`), regenerate
//!   (`base.py:116`), soft delete (`db/mixins.py:74-76`) — are full-row
//!   statements: INSERT lists every column, UPDATE rewrites every non-pk
//!   column. `updated_at` (`auto_now`, `db/mixins.py:20`) is advanced by
//!   the caller before the UPDATE.
//! * BUG (ported): `WebhookLog.webhook` (`db/models/webhook.py:68`) is a
//!   plain `UUIDField`, not a `ForeignKey` — the log predicate is a bare
//!   equality and rows orphan when their webhook is deleted.
//! * BUG (ported): `WebhookLog.response_status` stores mixed types (int
//!   status codes, `500`, `str(e)`) as text; the column stays `TEXT`.
//!
//! Boundaries (owned by sibling sub-issues, not ported here):
//!
//! * Serializer validation, DNS/SSRF guards, and the PATCH `context`
//!   quirk (`context={request: request}`, `base.py:84`) belong to
//!   PIDASHCONV-365 (FX-WEB-04/05).
//! * Permission outcomes belong to PIDASHCONV-436.
//! * The `soft_delete_related_objects.delay(...)` enqueue on delete
//!   (`db/mixins.py:78`) and the dispatch write path belong to
//!   PIDASHCONV-439 (FX-TSK-01).
//! * `409 {"error": "URL already exists for the workspace"}`
//!   (`base.py:31-35`) is raised from the `IntegrityError` message text,
//!   not from a pre-check query — there is no existence SELECT to port.

use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, JoinType, QueryFilter, QueryOrder, QuerySelect,
    RelationTrait, Select, Set,
};

use super::models_webhook::{generate_secret_key, webhook::Webhook, webhook_log::WebhookLog};
use crate::entity::{webhook_logs, webhooks, workspaces};

fn ts(value: chrono::DateTime<chrono::Utc>) -> sea_orm::prelude::DateTimeWithTimeZone {
    value.with_timezone(&chrono::FixedOffset::east_opt(0).expect("zero offset"))
}

fn opt_ts(
    value: Option<chrono::DateTime<chrono::Utc>>,
) -> Option<sea_orm::prelude::DateTimeWithTimeZone> {
    value.map(ts)
}

impl From<webhooks::Model> for Webhook {
    fn from(row: webhooks::Model) -> Self {
        let at = |at: sea_orm::prelude::DateTimeWithTimeZone| at.with_timezone(&chrono::Utc);
        Self {
            id: row.id,
            created_at: at(row.created_at),
            updated_at: at(row.updated_at),
            created_by_id: row.created_by_id,
            updated_by_id: row.updated_by_id,
            deleted_at: row.deleted_at.map(at),
            workspace_id: row.workspace_id,
            url: row.url,
            is_active: row.is_active,
            secret_key: row.secret_key,
            project: row.project,
            issue: row.issue,
            module: row.module,
            cycle: row.cycle,
            issue_comment: row.issue_comment,
            is_internal: row.is_internal,
            version: row.version,
        }
    }
}

impl From<webhook_logs::Model> for WebhookLog {
    fn from(row: webhook_logs::Model) -> Self {
        let at = |at: sea_orm::prelude::DateTimeWithTimeZone| at.with_timezone(&chrono::Utc);
        Self {
            id: row.id,
            created_at: at(row.created_at),
            updated_at: at(row.updated_at),
            created_by_id: row.created_by_id,
            updated_by_id: row.updated_by_id,
            deleted_at: row.deleted_at.map(at),
            workspace_id: row.workspace_id,
            webhook: row.webhook,
            event_type: row.event_type,
            request_method: row.request_method,
            request_headers: row.request_headers,
            request_body: row.request_body,
            response_status: row.response_status,
            response_headers: row.response_headers,
            response_body: row.response_body,
            retry_count: row.retry_count,
        }
    }
}

// ---------------------------------------------------------------------------
// Webhook list (base.py:39-58)
// ---------------------------------------------------------------------------

/// `Webhook.objects.filter(workspace__slug=slug)` (`base.py:41`), newest
/// first (`Meta.ordering`, `db/models/webhook.py:59`). The `workspace__slug`
/// traversal inner-joins `workspaces` (non-nullable FK).
pub fn webhook_list_query(workspace_slug: &str) -> Select<webhooks::Entity> {
    webhooks::Entity::find()
        .join(JoinType::InnerJoin, webhooks::Relation::Workspaces.def())
        .filter(webhooks::Column::DeletedAt.is_null())
        .filter(workspaces::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(workspace_slug.to_owned()))
        .order_by_desc(webhooks::Column::CreatedAt)
}

/// Newest-first live webhooks for one workspace slug
/// (`base.py:41-58`, `200`).
pub async fn fetch_webhook_list(
    db: &impl ConnectionTrait,
    workspace_slug: &str,
) -> Result<Vec<Webhook>, sea_orm::DbErr> {
    let rows = webhook_list_query(workspace_slug).all(db).await?;
    Ok(rows.into_iter().map(Webhook::from).collect())
}

// ---------------------------------------------------------------------------
// Webhook detail / patch lookup / delete lookup (base.py:59-76, :78-102, :104-108)
// ---------------------------------------------------------------------------

/// `Webhook.objects.get(workspace__slug=slug, pk=pk)` (`base.py:60,80`);
/// the delete path (`base.py:106`,
/// `get(pk=pk, workspace__slug=slug)`) carries the same predicates.
pub fn webhook_detail_query(
    webhook_id: uuid::Uuid,
    workspace_slug: &str,
) -> Select<webhooks::Entity> {
    webhooks::Entity::find()
        .join(JoinType::InnerJoin, webhooks::Relation::Workspaces.def())
        .filter(webhooks::Column::DeletedAt.is_null())
        .filter(webhooks::Column::Id.eq(webhook_id))
        .filter(workspaces::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(workspace_slug.to_owned()))
        .limit(1)
}

/// One live webhook by id scoped to a workspace slug, or `None` (the
/// view's 404 branch).
pub async fn fetch_webhook_detail(
    db: &impl ConnectionTrait,
    webhook_id: uuid::Uuid,
    workspace_slug: &str,
) -> Result<Option<Webhook>, sea_orm::DbErr> {
    let row = webhook_detail_query(webhook_id, workspace_slug)
        .one(db)
        .await?;
    Ok(row.map(Webhook::from))
}

// ---------------------------------------------------------------------------
// Webhook create (serializers/webhook.py:22-55, via base.py:25-28)
// ---------------------------------------------------------------------------

fn webhook_active(row: &Webhook) -> webhooks::ActiveModel {
    webhooks::ActiveModel {
        id: Set(row.id),
        created_at: Set(ts(row.created_at)),
        updated_at: Set(ts(row.updated_at)),
        created_by_id: Set(row.created_by_id),
        updated_by_id: Set(row.updated_by_id),
        deleted_at: Set(opt_ts(row.deleted_at)),
        workspace_id: Set(row.workspace_id),
        url: Set(row.url.clone()),
        is_active: Set(row.is_active),
        secret_key: Set(row.secret_key.clone()),
        project: Set(row.project),
        issue: Set(row.issue),
        module: Set(row.module),
        cycle: Set(row.cycle),
        issue_comment: Set(row.issue_comment),
        is_internal: Set(row.is_internal),
        version: Set(row.version.clone()),
    }
}

/// Insert one webhook row; returns the assigned id
/// (`201 + serializer.data`, `base.py:26-28`).
/// `Webhook.objects.create(**validated_data)` (`serializers/webhook.py:55`):
/// full-row INSERT; the id is client-assigned (`BaseModel.id` defaults to
/// `uuid4`, `db/models/base.py:18`).
pub async fn create_webhook(
    db: &impl ConnectionTrait,
    row: &Webhook,
) -> Result<uuid::Uuid, sea_orm::DbErr> {
    use sea_orm::ActiveModelTrait;
    let inserted = webhook_active(row).insert(db).await?;
    Ok(inserted.id)
}

// ---------------------------------------------------------------------------
// Full-row save: patch (base.py:99-101), regenerate (base.py:115-116),
// soft delete (db/mixins.py:74-76)
// ---------------------------------------------------------------------------

/// Rewrite one webhook row from the struct as Django's `save()` does:
/// DRF full-row UPDATE — every non-pk column is rewritten. The caller
/// advances `updated_at` (`auto_now`); `secret_key` / `deleted_at` travel
/// in the struct like any other field.
pub async fn update_webhook_full(
    db: &impl ConnectionTrait,
    row: &Webhook,
) -> Result<(), sea_orm::DbErr> {
    use sea_orm::ActiveModelTrait;
    webhook_active(row).update(db).await?;
    Ok(())
}

/// Stamp a fetched row for secret regenerate (`base.py:114-116`):
/// `webhook.secret_key = generate_token()` (`db/models/webhook.py:17-18`)
/// ahead of the full `save()`. `updated_at` advances (`auto_now`); the
/// caller persists with [`update_webhook_full`]. Single-statement
/// primitives stay generic over `sqlx::Executor` (one handle use each);
/// handlers compose fetch → stamp → update, as the view does lookup then
/// save. Returns the previous secret (tests assert rotation).
pub fn stamp_secret_regenerate(row: &mut Webhook) -> String {
    let previous = std::mem::replace(&mut row.secret_key, generate_secret_key());
    row.updated_at = chrono::Utc::now();
    previous
}

/// Stamp a fetched row for soft delete (`base.py:106-107`):
/// `SoftDeleteModel.delete(soft=True)` sets `deleted_at` then `save()`
/// (`db/mixins.py:72-79`). The caller persists with
/// [`update_webhook_full`]. The `soft_delete_related_objects.delay(...)`
/// enqueue (`mixins.py:78`) is a tasks-layer boundary (PIDASHCONV-439).
pub fn stamp_soft_delete(row: &mut Webhook) {
    let now = chrono::Utc::now();
    row.deleted_at = Some(now);
    row.updated_at = now;
}

// ---------------------------------------------------------------------------
// Webhook logs (base.py:121-126)
// ---------------------------------------------------------------------------

/// `WebhookLog.objects.filter(workspace__slug=slug, webhook=webhook_id)`
/// (`base.py:124`), newest first (`Meta.ordering`, `webhook.py:88`).
/// `webhook` is a plain UUID column, not a FK — the predicate is a bare
/// equality.
pub fn webhook_logs_query(
    workspace_slug: &str,
    webhook_id: uuid::Uuid,
) -> Select<webhook_logs::Entity> {
    webhook_logs::Entity::find()
        .join(
            JoinType::InnerJoin,
            webhook_logs::Relation::Workspaces.def(),
        )
        .filter(webhook_logs::Column::DeletedAt.is_null())
        .filter(workspaces::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(workspace_slug.to_owned()))
        .filter(webhook_logs::Column::Webhook.eq(webhook_id))
        .order_by_desc(webhook_logs::Column::CreatedAt)
}

/// Newest-first log rows for one webhook scoped to a workspace slug
/// (`base.py:124-126`, `200`).
/// `response_status` stays text: Django stores mixed types there
/// (ported BUG, see module docs).
pub async fn fetch_webhook_logs(
    db: &impl ConnectionTrait,
    workspace_slug: &str,
    webhook_id: uuid::Uuid,
) -> Result<Vec<WebhookLog>, sea_orm::DbErr> {
    let rows = webhook_logs_query(workspace_slug, webhook_id)
        .all(db)
        .await?;
    Ok(rows.into_iter().map(WebhookLog::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{DbBackend, QueryTrait};

    #[test]
    fn list_scopes_to_workspace_slug_newest_first() {
        let sql = webhook_list_query("acme")
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("FROM \"webhooks\""), "{sql}");
        assert!(sql.contains("INNER JOIN \"workspaces\""), "{sql}");
        assert!(
            sql.contains("\"webhooks\".\"workspace_id\" = \"workspaces\".\"id\""),
            "{sql}"
        );
        assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
        assert!(sql.contains("\"workspaces\".\"slug\" = 'acme'"), "{sql}");
        assert!(sql.contains("ORDER BY"), "{sql}");
        assert!(sql.contains("DESC"), "{sql}");
        assert!(!sql.contains("LIMIT"), "{sql}");
    }

    #[test]
    fn detail_is_scoped_get_without_ordering() {
        let sql = webhook_detail_query(uuid::Uuid::nil(), "acme")
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("FROM \"webhooks\""), "{sql}");
        assert!(sql.contains("INNER JOIN \"workspaces\""), "{sql}");
        assert!(
            sql.contains("\"id\" = '00000000-0000-0000-0000-000000000000'"),
            "{sql}"
        );
        assert!(sql.contains("\"slug\" = 'acme'"), "{sql}");
        // `get()` clears `Meta.ordering` and caps rows (domain convention:
        // `LIMIT 1` on unique predicates — same row as Django's `LIMIT 21`).
        assert!(!sql.contains("ORDER BY"), "{sql}");
        assert!(sql.contains("LIMIT 1"), "{sql}");
    }

    #[test]
    fn logs_filters_plain_uuid_column_newest_first() {
        let sql = webhook_logs_query("acme", uuid::Uuid::nil())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("FROM \"webhook_logs\""), "{sql}");
        assert!(sql.contains("INNER JOIN \"workspaces\""), "{sql}");
        // Plain UUID column, not a FK: bare equality, no `_id` remap.
        assert!(
            sql.contains("\"webhook\" = '00000000-0000-0000-0000-000000000000'"),
            "{sql}"
        );
        assert!(sql.contains("\"slug\" = 'acme'"), "{sql}");
        assert!(sql.contains("ORDER BY"), "{sql}");
        assert!(sql.contains("DESC"), "{sql}");
        assert!(!sql.contains("LIMIT"), "{sql}");
    }

    #[test]
    fn stamps_mutate_only_their_fields() {
        let ws = live_uuid(10);
        let mut row = live_webhook(
            live_uuid(20),
            ws,
            "https://a.example/hook",
            live_ts(1_700_000_100),
            None,
        );
        let previous = stamp_secret_regenerate(&mut row);
        assert_eq!(previous, "pi_dash_wh_0123456789abcdef0123456789abcdef");
        assert!(row.secret_key.starts_with("pi_dash_wh_"));
        assert_ne!(row.secret_key, previous);
        assert!(row.updated_at > live_ts(1_700_000_100));
        assert_eq!(row.url, "https://a.example/hook");
        assert!(row.deleted_at.is_none());
        stamp_soft_delete(&mut row);
        assert!(row.deleted_at.is_some());
        assert_eq!(row.deleted_at, Some(row.updated_at));
    }

    #[test]
    fn generated_secret_has_token_shape() {
        let secret = generate_secret_key();
        assert!(secret.starts_with("pi_dash_wh_"), "{secret}");
        let hex = secret.strip_prefix("pi_dash_wh_").expect("prefix");
        assert_eq!(hex.len(), 32, "{secret}");
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()), "{secret}");
        // Uniqueness per call (`uuid4().hex`, `webhook.py:17-18`).
        assert_ne!(secret, generate_secret_key());
    }

    // -- live scratch-DB tests (env-gated) -------------------------------

    /// Scratch Postgres for the Done-when verification. Unset (plain
    /// `cargo test`) skips these; CI sets no database either, so the suite
    /// stays green offline. Run with e.g.
    /// `export DATABASE_URL=postgresql:///pidash_428_scratch` for the real
    /// check (FX-WEB-01/02/03 SQL replay).
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
        "CREATE TEMPORARY TABLE webhooks (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, workspace_id UUID NOT NULL, url VARCHAR(1024) NOT NULL, is_active BOOLEAN NOT NULL, secret_key VARCHAR(255) NOT NULL, project BOOLEAN NOT NULL, issue BOOLEAN NOT NULL, module BOOLEAN NOT NULL, cycle BOOLEAN NOT NULL, issue_comment BOOLEAN NOT NULL, is_internal BOOLEAN NOT NULL, version VARCHAR(50) NOT NULL)",
        "CREATE TEMPORARY TABLE webhook_logs (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, created_by_id UUID, updated_by_id UUID, deleted_at TIMESTAMPTZ, workspace_id UUID NOT NULL, webhook UUID NOT NULL, event_type VARCHAR(255), request_method VARCHAR(10), request_headers TEXT, request_body TEXT, response_status TEXT, response_headers TEXT, response_body TEXT, retry_count SMALLINT NOT NULL)",
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

    fn live_webhook(
        id: uuid::Uuid,
        workspace_id: uuid::Uuid,
        url: &str,
        created_at: chrono::DateTime<chrono::Utc>,
        deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Webhook {
        Webhook {
            id,
            created_at,
            updated_at: created_at,
            created_by_id: None,
            updated_by_id: None,
            deleted_at,
            workspace_id,
            url: url.to_string(),
            is_active: true,
            secret_key: "pi_dash_wh_0123456789abcdef0123456789abcdef".to_string(),
            project: true,
            issue: false,
            module: false,
            cycle: false,
            issue_comment: false,
            is_internal: false,
            version: "v1".to_string(),
        }
    }

    async fn seed_workspace(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: uuid::Uuid,
        slug: &str,
    ) {
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, slug) VALUES ($1, $2, $2, 'Acme', $3)",
        )
        .bind(id)
        .bind(live_ts(1_700_000_000))
        .bind(slug)
        .execute(&mut **tx)
        .await
        .expect("seed workspace");
    }

    #[tokio::test]
    async fn live_crud_list_detail_create() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let ws = live_uuid(10);
        seed_workspace(&mut tx, ws, "acme").await;
        let other = live_uuid(11);
        seed_workspace(&mut tx, other, "other").await;
        // Two live rows (different ages), one soft-deleted, one foreign.
        for (suffix, workspace, url, ts, deleted) in [
            (20, ws, "https://a.example/hook", 1_700_000_100, None),
            (21, ws, "https://b.example/hook", 1_700_000_200, None),
            (
                22,
                ws,
                "https://gone.example/hook",
                1_700_000_300,
                Some(live_ts(1_700_000_400)),
            ),
            (23, other, "https://c.example/hook", 1_700_000_500, None),
        ] {
            let row = live_webhook(live_uuid(suffix), workspace, url, live_ts(ts), deleted);
            let conn = crate::orm_tx::TxConn::new(&mut tx);
            create_webhook(&conn, &row).await.expect("seed webhook");
        }
        // List: live rows of this workspace only, newest first.
        let conn = crate::orm_tx::TxConn::new(&mut tx);
        let list = fetch_webhook_list(&conn, "acme").await.expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].url, "https://b.example/hook");
        assert_eq!(list[1].url, "https://a.example/hook");
        // Detail: hit, soft-deleted miss, cross-workspace miss.
        let hit = fetch_webhook_detail(&conn, live_uuid(20), "acme")
            .await
            .expect("detail")
            .expect("row");
        assert_eq!(hit.url, "https://a.example/hook");
        assert!(hit.project);
        assert!(fetch_webhook_detail(&conn, live_uuid(22), "acme")
            .await
            .expect("deleted")
            .is_none());
        assert!(fetch_webhook_detail(&conn, live_uuid(20), "other")
            .await
            .expect("scoped")
            .is_none());
        assert!(fetch_webhook_detail(&conn, live_uuid(23), "acme")
            .await
            .expect("foreign")
            .is_none());
        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn live_regenerate_rotates_secret() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let ws = live_uuid(10);
        seed_workspace(&mut tx, ws, "acme").await;
        let id = live_uuid(20);
        let before = live_webhook(
            id,
            ws,
            "https://a.example/hook",
            live_ts(1_700_000_100),
            None,
        );
        let conn = crate::orm_tx::TxConn::new(&mut tx);
        create_webhook(&conn, &before).await.expect("seed");
        let mut after = fetch_webhook_detail(&conn, id, "acme")
            .await
            .expect("lookup")
            .expect("row");
        let previous = stamp_secret_regenerate(&mut after);
        assert_eq!(previous, before.secret_key);
        // TIMESTAMPTZ stores microseconds; truncate the stamp to what the
        // database keeps so the round-trip assertion below is exact.
        after.updated_at = chrono::DateTime::from_timestamp(
            after.updated_at.timestamp(),
            after.updated_at.timestamp_subsec_micros() * 1000,
        )
        .expect("microsecond truncation stays in range");
        update_webhook_full(&conn, &after).await.expect("save");
        assert_ne!(after.secret_key, before.secret_key);
        assert!(after.secret_key.starts_with("pi_dash_wh_"));
        assert_eq!(after.secret_key.len(), "pi_dash_wh_".len() + 32);
        assert!(after.updated_at > before.updated_at);
        // Untouched columns round-trip; the row reads back identically.
        assert_eq!(after.url, before.url);
        assert_eq!(after.workspace_id, before.workspace_id);
        let reread = fetch_webhook_detail(&conn, id, "acme")
            .await
            .expect("reread")
            .expect("row");
        assert_eq!(reread, after);
        // Miss returns None (the 404 branch).
        assert!(fetch_webhook_detail(&conn, live_uuid(99), "acme")
            .await
            .expect("miss")
            .is_none());
        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn live_soft_delete_hides_row() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let ws = live_uuid(10);
        seed_workspace(&mut tx, ws, "acme").await;
        let id = live_uuid(20);
        let conn = crate::orm_tx::TxConn::new(&mut tx);
        create_webhook(
            &conn,
            &live_webhook(
                id,
                ws,
                "https://a.example/hook",
                live_ts(1_700_000_100),
                None,
            ),
        )
        .await
        .expect("seed");
        let mut stamped = fetch_webhook_detail(&conn, id, "acme")
            .await
            .expect("lookup")
            .expect("row");
        stamp_soft_delete(&mut stamped);
        update_webhook_full(&conn, &stamped).await.expect("save");
        assert!(stamped.deleted_at.is_some());
        // The row leaves the default-manager reads (list + detail).
        assert!(fetch_webhook_list(&conn, "acme")
            .await
            .expect("list")
            .is_empty());
        assert!(fetch_webhook_detail(&conn, id, "acme")
            .await
            .expect("detail")
            .is_none());
        // ... but the marker is a stamp, not a removal (full save kept it).
        let (count, marker): (i64, bool) = sqlx::query_as(
            "SELECT COUNT(*), BOOL_AND(deleted_at IS NOT NULL) FROM webhooks WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .expect("raw check");
        assert_eq!((count, marker), (1, true));
        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn live_logs_filter_and_order() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let mut tx = live_tx(&pool).await;
        let ws = live_uuid(10);
        seed_workspace(&mut tx, ws, "acme").await;
        let hook = live_uuid(20);
        let foreign_hook = live_uuid(21);
        let conn = crate::orm_tx::TxConn::new(&mut tx);
        create_webhook(
            &conn,
            &live_webhook(
                hook,
                ws,
                "https://a.example/hook",
                live_ts(1_700_000_100),
                None,
            ),
        )
        .await
        .expect("seed hook");
        let seed_log = "INSERT INTO webhook_logs (id, created_at, updated_at, workspace_id, webhook, event_type, request_method, request_headers, request_body, response_status, response_headers, response_body, retry_count) VALUES ($1, $2, $2, $3, $4, 'issue', 'create', '{}', '{}', $5, '', '', 0)";
        // Older int-status row, newer str(e)-status row (the mixed-type
        // BUG round-trips as text), one row for another webhook.
        for (suffix, target, ts, status) in [
            (30, hook, 1_700_000_110, "200"),
            (31, hook, 1_700_000_120, "ConnectionError('refused')"),
            (32, foreign_hook, 1_700_000_130, "200"),
        ] {
            sqlx::query(seed_log)
                .bind(live_uuid(suffix))
                .bind(live_ts(ts))
                .bind(ws)
                .bind(target)
                .bind(status)
                .execute(&mut *tx)
                .await
                .expect("seed log");
        }
        let conn = crate::orm_tx::TxConn::new(&mut tx);
        let logs = fetch_webhook_logs(&conn, "acme", hook).await.expect("logs");
        assert_eq!(logs.len(), 2);
        assert_eq!(
            logs[0].response_status.as_deref(),
            Some("ConnectionError('refused')")
        );
        assert_eq!(logs[0].retry_count, 0);
        assert_eq!(logs[1].response_status.as_deref(), Some("200"));
        // The plain-UUID predicate orphans: logs survive their webhook.
        sqlx::query("DELETE FROM webhooks WHERE id = $1")
            .bind(hook)
            .execute(&mut *tx)
            .await
            .expect("hard delete hook");
        let conn = crate::orm_tx::TxConn::new(&mut tx);
        assert_eq!(
            fetch_webhook_logs(&conn, "acme", hook)
                .await
                .expect("orphans")
                .len(),
            2
        );
        tx.rollback().await.expect("rollback");
    }
}
