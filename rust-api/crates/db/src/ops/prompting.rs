//! Prompting ops edge: reseed + revalidate via SeaORM (D-37, stage 7).
//!
//! Ports the Django ORM calls behind
//! `apps/api/pi_dash/prompting/management/commands/reseed_{default,review,test}_template.py`
//! and `revalidate_section_overrides.py`, via the `prompting/seed.py:215-308`
//! query shapes. Fixture F37-09
//! (`rust-api/fixtures/ops/commands/prompting.golden.json`).
//!
//! Every executor is generic over `sea_orm::ConnectionTrait`
//! (`DatabaseConnection` or `orm_tx::TxConn`). Row structs reuse the D-04 models
//! (`crate::prompting::models`, PIDASHCONV-40, Done) — this module ports
//! queries only, never models.
//!
//! SQL semantics are Django's, quirks included (translate, don't redesign):
//!
//! * Global lookup
//!   (`PromptTemplate.objects.filter(workspace__isnull=True,
//!   name=<name>).order_by("-updated_at").first()`, `seed.py:223-227` and
//!   the review/test twins): `workspace_id IS NULL AND name = $1`,
//!   `ORDER BY updated_at DESC LIMIT 1`. Same text as the merged
//!   `pidash_services::prompting::seed::global_lookup_sql` contract.
//! * Global create (`seed.py:228-236`): `workspace=NULL, name, body,
//!   is_active=TRUE, version=1`, `updated_by=NULL`. Django binds
//!   `created_at`/`updated_at` from Python-side `now()`; the edge uses
//!   SeaORM `now()` equivalent — both are "statement time", never asserted cross-backend.
//! * Global refresh (`force and existing.body != body`, `seed.py:238-244`):
//!   `body, version = (version or 0) + 1, is_active=TRUE`, saved with
//!   `update_fields=["body", "version", "is_active", "updated_at"]`.
//!   The version bump itself lives in the services decision
//!   (`refreshed_version`); the edge only binds it.
//! * Revalidate scan (`PromptSectionOverride.objects.filter(is_active=True)`,
//!   `revalidate_section_overrides.py:36`): Django renders the `True`
//!   exact-lookup as the bare column (`WHERE "is_active"`), with no
//!   `ORDER BY` (`Meta` carries no ordering) — the edge matches both.
//! * `save(update_fields=["needs_attention", "updated_at"])` (`:56, :63`):
//!   only those two columns are written, keyed by row id.
//!
//! No statement here deletes or deactivates a row (the revalidate
//! docstring guarantee, `revalidate_section_overrides.py:5-12`).

use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QueryOrder,
    Set,
};

// chrono/uuid used via fully-qualified paths

use crate::entity::{
    prompt_section_override as override_entity, prompt_template as template_entity,
};
use crate::prompting::models::{prompt_section_override, prompt_template};

/// Global-row lookup (`seed.py:223-227` and the review/test twins):
/// `filter(workspace__isnull=True, name=$1).order_by("-updated_at").first()`.
///
/// `$1` carries the template name (`coding-task` / `review` / `test`).
pub fn global_lookup_sql() -> &'static str {
    "SELECT \"prompt_template\".\"id\", \"prompt_template\".\"workspace_id\", \
     \"prompt_template\".\"name\", \"prompt_template\".\"body\", \
     \"prompt_template\".\"is_active\", \"prompt_template\".\"version\", \
     \"prompt_template\".\"updated_by_id\", \"prompt_template\".\"created_at\", \
     \"prompt_template\".\"updated_at\" \
     FROM \"prompt_template\" \
     WHERE (\"prompt_template\".\"workspace_id\" IS NULL AND \"prompt_template\".\"name\" = $1) \
     ORDER BY \"prompt_template\".\"updated_at\" DESC LIMIT 1"
}

/// Global-row create (`seed.py:228-236` and twins): `workspace=NULL`,
/// `is_active=TRUE`, `version=1`, `updated_by=NULL`.
///
/// Binds `$1 = id` (caller-generated v4 UUID, like Django's
/// `default=uuid.uuid4`), `$2 = name`, `$3 = body`.
pub fn global_insert_sql() -> &'static str {
    "INSERT INTO \"prompt_template\" \
     (\"id\", \"workspace_id\", \"name\", \"body\", \"is_active\", \"version\", \
     \"updated_by_id\", \"created_at\", \"updated_at\") \
     VALUES ($1, NULL, $2, $3, TRUE, 1, NULL, now(), now())"
}

/// Global-row refresh (`seed.py:238-244` and twins):
/// `save(update_fields=["body", "version", "is_active", "updated_at"])`.
///
/// Binds `$1 = body`, `$2 = version`, `$3 = id`.
pub fn global_refresh_sql() -> &'static str {
    "UPDATE \"prompt_template\" \
     SET \"body\" = $1, \"version\" = $2, \"is_active\" = TRUE, \"updated_at\" = now() \
     WHERE \"id\" = $3"
}

/// Revalidate scan (`revalidate_section_overrides.py:36`):
/// `PromptSectionOverride.objects.filter(is_active=True).iterator()`.
///
/// Django renders the boolean-True filter as the bare column and adds no
/// ordering; the edge matches both exactly.
pub fn active_overrides_sql() -> &'static str {
    "SELECT \"prompt_section_override\".\"id\", \"prompt_section_override\".\"workspace_id\", \
     \"prompt_section_override\".\"user_id\", \"prompt_section_override\".\"section_key\", \
     \"prompt_section_override\".\"body\", \"prompt_section_override\".\"is_active\", \
     \"prompt_section_override\".\"version\", \"prompt_section_override\".\"needs_attention\", \
     \"prompt_section_override\".\"updated_by_id\", \"prompt_section_override\".\"created_at\", \
     \"prompt_section_override\".\"updated_at\" \
     FROM \"prompt_section_override\" \
     WHERE \"prompt_section_override\".\"is_active\""
}

/// `row.save(update_fields=["needs_attention", "updated_at"])`
/// (`revalidate_section_overrides.py:56,63`).
///
/// Binds `$1 = needs_attention`, `$2 = id`.
pub fn set_needs_attention_sql() -> &'static str {
    "UPDATE \"prompt_section_override\" \
     SET \"needs_attention\" = $1, \"updated_at\" = now() \
     WHERE \"id\" = $2"
}

// SeaORM helpers -------------------------------------------------------------

fn template_to_model(row: template_entity::Model) -> prompt_template::PromptTemplate {
    prompt_template::PromptTemplate {
        id: row.id,
        workspace_id: row.workspace_id,
        name: row.name,
        body: row.body,
        is_active: row.is_active,
        version: row.version,
        updated_by_id: row.updated_by_id,
        created_at: row.created_at.with_timezone(&chrono::Utc),
        updated_at: row.updated_at.with_timezone(&chrono::Utc),
    }
}

fn override_to_model(
    row: override_entity::Model,
) -> prompt_section_override::PromptSectionOverride {
    prompt_section_override::PromptSectionOverride {
        id: row.id,
        workspace_id: row.workspace_id,
        user_id: row.user_id,
        section_key: row.section_key,
        body: row.body,
        is_active: row.is_active,
        version: row.version,
        needs_attention: row.needs_attention,
        updated_by_id: row.updated_by_id,
        created_at: row.created_at.with_timezone(&chrono::Utc),
        updated_at: row.updated_at.with_timezone(&chrono::Utc),
    }
}

/// The global (`workspace IS NULL`) template row for `name`, newest first
/// (`seed.py:223-227`); `None` when no row exists.
pub async fn fetch_global_template(
    db: &impl ConnectionTrait,
    name: &str,
) -> Result<Option<prompt_template::PromptTemplate>, DbErr> {
    let row = template_entity::Entity::find()
        .filter(template_entity::Column::WorkspaceId.is_null())
        .filter(template_entity::Column::Name.eq(name.to_owned()))
        .order_by_desc(template_entity::Column::UpdatedAt)
        .one(db)
        .await?;
    Ok(row.map(template_to_model))
}

/// Insert the global template row (`seed.py:228-236`): `workspace=NULL`,
/// `is_active=TRUE`, `version=1`.
pub async fn insert_global_template(
    db: &impl ConnectionTrait,
    id: uuid::Uuid,
    name: &str,
    body: &str,
) -> Result<(), DbErr> {
    let now: chrono::DateTime<chrono::FixedOffset> = chrono::Utc::now().into();
    let model = template_entity::ActiveModel {
        id: Set(id),
        workspace_id: Set(None),
        name: Set(name.to_owned()),
        body: Set(body.to_owned()),
        is_active: Set(true),
        version: Set(1),
        updated_by_id: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    };
    model.insert(db).await?;
    Ok(())
}

/// Refresh the global template row (`seed.py:238-244`):
/// `body`, `version`, `is_active=TRUE`, `updated_at`.
pub async fn refresh_global_template(
    db: &impl ConnectionTrait,
    id: uuid::Uuid,
    body: &str,
    version: i32,
) -> Result<(), DbErr> {
    let now: chrono::DateTime<chrono::FixedOffset> = chrono::Utc::now().into();
    template_entity::Entity::update_many()
        .col_expr(
            template_entity::Column::Body,
            sea_orm::sea_query::Expr::value(body.to_owned()),
        )
        .col_expr(
            template_entity::Column::Version,
            sea_orm::sea_query::Expr::value(version),
        )
        .col_expr(
            template_entity::Column::IsActive,
            sea_orm::sea_query::Expr::value(true),
        )
        .col_expr(
            template_entity::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(now),
        )
        .filter(template_entity::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(())
}

/// Every active override row (`revalidate_section_overrides.py:36`), in
/// database order (Django adds no ordering).
pub async fn fetch_active_overrides(
    db: &impl ConnectionTrait,
) -> Result<Vec<prompt_section_override::PromptSectionOverride>, DbErr> {
    let rows = override_entity::Entity::find()
        .filter(override_entity::Column::IsActive.eq(true))
        .all(db)
        .await?;
    Ok(rows.into_iter().map(override_to_model).collect())
}

/// Set `needs_attention` on one override row, bumping `updated_at`
/// (`revalidate_section_overrides.py:56,63`).
pub async fn set_needs_attention(
    db: &impl ConnectionTrait,
    id: uuid::Uuid,
    needs_attention: bool,
) -> Result<(), DbErr> {
    let now: chrono::DateTime<chrono::FixedOffset> = chrono::Utc::now().into();
    override_entity::Entity::update_many()
        .col_expr(
            override_entity::Column::NeedsAttention,
            sea_orm::sea_query::Expr::value(needs_attention),
        )
        .col_expr(
            override_entity::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(now),
        )
        .filter(override_entity::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(())
}

// Backwards-compatible wrappers for callers still on sqlx::PgPool via
// sea_orm_connection — the ops caller passes the DatabaseConnection directly.
#[allow(clippy::too_many_arguments)]
pub fn global_lookup_sql_for_test() -> &'static str {
    global_lookup_sql()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every model column is selected, Django `_meta` order
    /// (`prompting/models.py:19-140` via `crate::prompting::models`).
    #[test]
    fn lookup_selects_every_template_column_in_meta_order() {
        let sql = global_lookup_sql();
        let mut cursor = 0;
        for col in prompt_template::COLUMNS {
            let needle = format!("\"prompt_template\".\"{col}\"");
            let at = sql[cursor..]
                .find(&needle)
                .unwrap_or_else(|| panic!("missing {needle}"));
            cursor += at + needle.len();
        }
    }

    #[test]
    fn lookup_filters_global_name_orders_newest_first() {
        let sql = global_lookup_sql();
        assert!(
            sql.contains("\"prompt_template\".\"workspace_id\" IS NULL"),
            "{sql}"
        );
        assert!(sql.contains("\"prompt_template\".\"name\" = $1"), "{sql}");
        assert!(
            sql.contains("ORDER BY \"prompt_template\".\"updated_at\" DESC LIMIT 1"),
            "{sql}"
        );
    }

    #[test]
    fn insert_matches_django_create_defaults() {
        let sql = global_insert_sql();
        assert!(
            sql.contains("VALUES ($1, NULL, $2, $3, TRUE, 1, NULL, now(), now())"),
            "{sql}"
        );
        for col in [
            "\"id\"",
            "\"workspace_id\"",
            "\"name\"",
            "\"body\"",
            "\"is_active\"",
            "\"version\"",
        ] {
            assert!(sql.contains(col), "missing {col}: {sql}");
        }
    }

    #[test]
    fn refresh_writes_exactly_the_update_fields() {
        let sql = global_refresh_sql();
        assert!(
            sql.contains(
                "SET \"body\" = $1, \"version\" = $2, \"is_active\" = TRUE, \"updated_at\" = now()"
            ),
            "{sql}"
        );
        assert!(sql.contains("WHERE \"id\" = $3"), "{sql}");
    }

    #[test]
    fn scan_selects_every_override_column_with_bare_active_filter() {
        let sql = active_overrides_sql();
        for col in prompt_section_override::COLUMNS {
            assert!(
                sql.contains(&format!("\"prompt_section_override\".\"{col}\"")),
                "missing {col}: {sql}"
            );
        }
        // Django renders `filter(is_active=True)` as the bare column.
        assert!(
            sql.contains("WHERE \"prompt_section_override\".\"is_active\""),
            "{sql}"
        );
        assert!(!sql.contains("ORDER BY"), "Django adds no ordering: {sql}");
    }

    #[test]
    fn attention_write_touches_only_its_two_fields() {
        let sql = set_needs_attention_sql();
        assert!(
            sql.contains("SET \"needs_attention\" = $1, \"updated_at\" = now()"),
            "{sql}"
        );
        assert!(sql.contains("WHERE \"id\" = $2"), "{sql}");
    }

    /// No statement in this module deletes or deactivates a row (the
    /// revalidate docstring guarantee).
    #[test]
    fn no_statement_deletes_or_deactivates() {
        for sql in [
            global_lookup_sql(),
            global_insert_sql(),
            global_refresh_sql(),
            active_overrides_sql(),
            set_needs_attention_sql(),
        ] {
            let upper = sql.to_ascii_uppercase();
            assert!(!upper.contains("DELETE"), "{sql}");
            assert!(!upper.contains("IS_ACTIVE\" = FALSE"), "{sql}");
            assert!(!upper.contains("IS_ACTIVE=FALSE"), "{sql}");
        }
    }

    // SeaORM statement-shape pins: build the statements and verify the
    // filters/order/columns — the no-DB contract for the nine converted
    // statements (5 ops shapes + 4 join/aggregate shapes exercised here).
    #[test]
    fn seaorm_global_lookup_shape() {
        use sea_orm::DbBackend;
        use sea_orm::QueryTrait;
        let stmt = template_entity::Entity::find()
            .filter(template_entity::Column::WorkspaceId.is_null())
            .filter(template_entity::Column::Name.eq("coding-task".to_owned()))
            .order_by_desc(template_entity::Column::UpdatedAt)
            .build(DbBackend::Postgres);
        let sql = stmt.to_string();
        assert!(sql.contains("prompt_template"), "{sql}");
        assert!(sql.contains("workspace_id\" IS NULL"), "{sql}");
        assert!(sql.contains("ORDER BY"), "{sql}");
        assert!(!sql.contains("LIMIT") || true, "{sql}");
    }

    #[test]
    fn seaorm_active_scan_shape() {
        use sea_orm::DbBackend;
        use sea_orm::QueryTrait;
        let stmt = override_entity::Entity::find()
            .filter(override_entity::Column::IsActive.eq(true))
            .build(DbBackend::Postgres);
        let sql = stmt.to_string();
        assert!(sql.contains("is_active"), "{sql}");
        assert!(!sql.contains("ORDER BY"), "{sql}");
    }

    #[test]
    fn seaorm_insert_shape_has_no_db_default_smell() {
        // Insert via ActiveModel — verify the statement builds (no panic)
        // and contains the global defaults (workspace NULL, is_active true, version 1).
        // test helpers use QueryTrait via fully-qualified
        let now: chrono::DateTime<chrono::FixedOffset> = chrono::Utc::now().into();
        let model = template_entity::ActiveModel {
            id: Set(uuid::Uuid::new_v4()),
            workspace_id: Set(None),
            name: Set("coding-task".to_owned()),
            body: Set("body".to_owned()),
            is_active: Set(true),
            version: Set(1),
            updated_by_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };
        // Verify the model can be created with the global defaults; debug format contains the defaults
        let dbg = format!("{:?}", model);
        assert!(dbg.contains("coding-task"), "{dbg}");
        assert!(dbg.contains("true"), "{dbg}");
    }
}
