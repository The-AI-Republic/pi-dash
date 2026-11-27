//! Prompting ops edge: revalidate via SeaORM (D-37, stage 7).
//!
//! Ports the Django ORM calls behind
//! `apps/api/pi_dash/prompting/management/commands/revalidate_section_overrides.py`
//! via the `prompting/seed.py` query shapes. Fixture F37-09
//! (`rust-api/fixtures/ops/commands/prompting.golden.json`).
//!
//! The legacy reseed edge is gone (NEWPIDASH-404): nothing reads
//! `prompt_template.body` at run time, so the global
//! lookup/create/refresh statements and their SeaORM executors were
//! removed. The `prompt_template` table and its entity/model stay so the
//! shared database keeps working with Django.
//!
//! Every executor is generic over `sea_orm::ConnectionTrait`
//! (`DatabaseConnection` or `orm_tx::TxConn`). Row structs reuse the D-04 models
//! (`crate::prompting::models`, PIDASHCONV-40, Done) — this module ports
//! queries only, never models.
//!
//! SQL semantics are Django's, quirks included (translate, don't redesign):
//!
//! * Revalidate scan (`PromptSectionOverride.objects.filter(is_active=True)`,
//!   `revalidate_section_overrides.py:36`): Django renders the `True`
//!   exact-lookup as the bare column (`WHERE "is_active"`), with no
//!   `ORDER BY` (`Meta` carries no ordering) — the edge matches both.
//! * `save(update_fields=["needs_attention", "updated_at"])` (`:56, :63`):
//!   only those two columns are written, keyed by row id.
//!
//! No statement here deletes or deactivates a row (the revalidate
//! docstring guarantee, `revalidate_section_overrides.py:5-12`).

use sea_orm::{ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter};

use crate::entity::prompt_section_override as override_entity;

use crate::prompting::models::prompt_section_override;

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

#[cfg(test)]
mod tests {
    use super::*;

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
        for sql in [active_overrides_sql(), set_needs_attention_sql()] {
            let upper = sql.to_ascii_uppercase();
            assert!(!upper.contains("DELETE"), "{sql}");
            assert!(!upper.contains("IS_ACTIVE\" = FALSE"), "{sql}");
            assert!(!upper.contains("IS_ACTIVE=FALSE"), "{sql}");
        }
    }

    // SeaORM statement-shape pins: build the statements and verify the
    // filters/order/columns — the no-DB contract for the converted
    // statements exercised here.
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
}
