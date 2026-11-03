//! Prompting ops SQL edge: reseed + revalidate statements (D-37, stage 7).
//!
//! Ports the Django ORM calls behind
//! `apps/api/pi_dash/prompting/management/commands/reseed_{default,review,test}_template.py`
//! and `revalidate_section_overrides.py`, via the `prompting/seed.py:215-308`
//! query shapes. Fixture F37-09
//! (`rust-api/fixtures/ops/commands/prompting.golden.json`).
//!
//! Fixed statements are string constants executed with runtime `sqlx::query`
//! (no `query!` macros: there is no build-time database, same as the merged
//! `license/queries` precedent). Every executor is generic over
//! `sqlx::Executor`. Row structs reuse the D-04 models
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
//!   `now()` — both are "statement time", never asserted cross-backend.
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

use sqlx::postgres::PgRow;
use sqlx::Row;

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

/// Map one full `prompt_template` row (column names, order-independent).
pub fn map_prompt_template_row(
    row: &PgRow,
) -> Result<prompt_template::PromptTemplate, sqlx::Error> {
    Ok(prompt_template::PromptTemplate {
        id: row.try_get("id")?,
        workspace_id: row.try_get("workspace_id")?,
        name: row.try_get("name")?,
        body: row.try_get("body")?,
        is_active: row.try_get("is_active")?,
        version: row.try_get("version")?,
        updated_by_id: row.try_get("updated_by_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

/// Map one full `prompt_section_override` row (column names, order-independent).
pub fn map_prompt_section_override_row(
    row: &PgRow,
) -> Result<prompt_section_override::PromptSectionOverride, sqlx::Error> {
    Ok(prompt_section_override::PromptSectionOverride {
        id: row.try_get("id")?,
        workspace_id: row.try_get("workspace_id")?,
        user_id: row.try_get("user_id")?,
        section_key: row.try_get("section_key")?,
        body: row.try_get("body")?,
        is_active: row.try_get("is_active")?,
        version: row.try_get("version")?,
        needs_attention: row.try_get("needs_attention")?,
        updated_by_id: row.try_get("updated_by_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

/// The global (`workspace IS NULL`) template row for `name`, newest first
/// (`seed.py:223-227`); `None` when no row exists.
pub async fn fetch_global_template<'e, E>(
    ex: E,
    name: &str,
) -> Result<Option<prompt_template::PromptTemplate>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<PgRow> = sqlx::query(global_lookup_sql())
        .bind(name)
        .fetch_optional(ex)
        .await?;
    row.map(|row| map_prompt_template_row(&row)).transpose()
}

/// Insert the global template row (`seed.py:228-236`): `workspace=NULL`,
/// `is_active=TRUE`, `version=1`.
pub async fn insert_global_template<'e, E>(
    ex: E,
    id: uuid::Uuid,
    name: &str,
    body: &str,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(global_insert_sql())
        .bind(id)
        .bind(name)
        .bind(body)
        .execute(ex)
        .await?;
    Ok(())
}

/// Refresh the global template row (`seed.py:238-244`):
/// `body`, `version`, `is_active=TRUE`, `updated_at`.
pub async fn refresh_global_template<'e, E>(
    ex: E,
    id: uuid::Uuid,
    body: &str,
    version: i32,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(global_refresh_sql())
        .bind(body)
        .bind(version)
        .bind(id)
        .execute(ex)
        .await?;
    Ok(())
}

/// Every active override row (`revalidate_section_overrides.py:36`), in
/// database order (Django adds no ordering).
pub async fn fetch_active_overrides<'e, E>(
    ex: E,
) -> Result<Vec<prompt_section_override::PromptSectionOverride>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<PgRow> = sqlx::query(active_overrides_sql()).fetch_all(ex).await?;
    rows.iter().map(map_prompt_section_override_row).collect()
}

/// Set `needs_attention` on one override row, bumping `updated_at`
/// (`revalidate_section_overrides.py:56,63`).
pub async fn set_needs_attention<'e, E>(
    ex: E,
    id: uuid::Uuid,
    needs_attention: bool,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(set_needs_attention_sql())
        .bind(needs_attention)
        .bind(id)
        .execute(ex)
        .await?;
    Ok(())
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
}
