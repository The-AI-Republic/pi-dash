//! Runner lookup queries for `DELETE /api/v1/runners/<id>/` (D-22, V1CLIAUTH-F3).
//!
//! Ports the units PIDASHCONV-533 owns:
//!
//! * Q1 by-pk lookup: `Runner.objects.filter(pk=runner_id).first()`
//!   (`apps/api/pi_dash/api/views/runner.py:45`) — no deleted filter
//!   (there is no `deleted_at` column), no join.
//! * Q2 guard-facts read: `can_view_runner` reads `runner.visibility` +
//!   `runner.owner_id` (`runner/services/permissions.py:74-75`) and
//!   `can_manage_runner` re-reads the same attributes (`:135-136`) off
//!   the Q1 row in Python — **no second query**. The workspace-admin
//!   branch (`:137`, `is_workspace_admin` over `workspace_members`) is
//!   unreachable on this path (it requires view-True with a non-PRIVATE
//!   visibility, and view-True requires PRIVATE), so the
//!   `workspace_members` query never fires here.
//!
//! Fixture source of truth: `rust-api/fixtures/v1_cli_auth/`
//! `V1CLIAUTH-F3.lookup.sql` + `V1CLIAUTH-F3.lookup.rows.json`
//! (PIDASHCONV-526). The `#[cfg(test)]` suite asserts the SQL text and
//! the row shape against those fixtures.
//!
//! SQL execution is SeaORM (`runner::Entity::find_by_id`); the projected
//! column set comes from the entity, whose columns match the recorded
//! Django SQL exactly. The old `sqlx` string builder is gone; the unit
//! test pins the built statement shape so transcription drift fails the
//! build.
//!
//! # Port shape vs Django shape (same semantics)
//!
//! Django's Q1 carries `ORDER BY "runner"."last_heartbeat_at" DESC,
//! "runner"."created_at" DESC LIMIT 1` (`Meta.ordering` + `.first()`).
//! The ordering is semantically vacuous on a pk lookup (at most one row
//! matches), so the port is the plain by-pk lookup, and
//! [`runner_lookup::find_by_pk`] uses `one` for the `.first()` LIMIT-1
//! cardinality.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * None observed on this path (fixture TRACE "Ported bugs"). The 403
//!   branch (`api/views/runner.py:50-51`) and the `:137`
//!   owner-or-admin line are unreachable-but-by-design and stay ported;
//!   they live in the handler layer (PIDASHCONV-538), not here.

use chrono::Utc;
use sea_orm::EntityTrait;

use super::models::runner::Runner;
use crate::entity::runner as runner_entity;

/// By-pk lookup + guard-facts projection (`views/runner.py:45-51`).
pub mod runner_lookup {
    use super::*;

    /// Django table name (`Meta.db_table`, `runner/models.py:479`).
    pub const TABLE: &str = "runner";

    /// Guard facts for `can_view_runner` / `can_manage_runner`
    /// (`permissions.py:74-75, :135-136`): the `(visibility, owner_id)`
    /// projection off the Q1 row. No `workspace_id`: its only consumer
    /// (`:137` `is_workspace_admin`) is unreachable on this path.
    /// Feeds the read-only `pidash_auth::permissions::runner` kernel.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct GuardFacts {
        pub visibility: i16,
        pub owner_id: uuid::Uuid,
    }

    /// Q2 is a projection, not a query: read the guard facts off the
    /// already-fetched row.
    pub fn guard_facts(runner: &Runner) -> GuardFacts {
        GuardFacts {
            visibility: runner.visibility,
            owner_id: runner.owner_id,
        }
    }

    fn to_utc(t: chrono::DateTime<chrono::FixedOffset>) -> chrono::DateTime<Utc> {
        t.with_timezone(&Utc)
    }

    /// Map the SeaORM entity row into [`Runner`], converting the
    /// timestamptz columns to `Utc` (the read-model type).
    pub fn runner_from_model(model: runner_entity::Model) -> Runner {
        Runner {
            id: model.id,
            owner_id: model.owner_id,
            workspace_id: model.workspace_id,
            dev_machine_id: model.dev_machine_id,
            pod_id: model.pod_id,
            name: model.name,
            host_label: model.host_label,
            provisioning: model.provisioning,
            visibility: model.visibility,
            refresh_token_hash: model.refresh_token_hash,
            refresh_token_fingerprint: model.refresh_token_fingerprint,
            refresh_token_generation: model.refresh_token_generation,
            previous_refresh_token_hash: model.previous_refresh_token_hash,
            access_token_signing_key_version: model.access_token_signing_key_version,
            enrollment_token_hash: model.enrollment_token_hash,
            enrollment_token_fingerprint: model.enrollment_token_fingerprint,
            enrolled_at: model.enrolled_at.map(to_utc),
            capabilities: model.capabilities,
            status: model.status,
            os: model.os,
            arch: model.arch,
            runner_version: model.runner_version,
            dev_metadata: model.dev_metadata,
            protocol_version: model.protocol_version,
            last_heartbeat_at: model.last_heartbeat_at.map(to_utc),
            created_at: to_utc(model.created_at),
            updated_at: to_utc(model.updated_at),
            revoked_at: model.revoked_at.map(to_utc),
            revoked_reason: model.revoked_reason,
            paused_at: model.paused_at.map(to_utc),
            paused_by_id: model.paused_by_id,
        }
    }

    /// `Runner.objects.filter(pk=runner_id).first()`
    /// (`views/runner.py:45`): `None` when the id is unknown (handler
    /// 404). The by-pk entity lookup carries the `.first()` LIMIT-1
    /// cardinality; Django's vacuous `ORDER BY` rides no statement.
    pub async fn find_by_pk(
        pool: &sqlx::PgPool,
        runner_id: uuid::Uuid,
    ) -> Result<Option<Runner>, sea_orm::DbErr> {
        let db = crate::pool::sea_orm_connection(pool);
        let model = runner_entity::Entity::find_by_id(runner_id)
            .one(&db)
            .await?;
        Ok(model.map(runner_from_model))
    }

    /// The by-pk statement (`find_by_pk` without a database): pins the
    /// shape the unit tests assert.
    pub fn by_pk_stmt(runner_id: uuid::Uuid) -> sea_orm::sea_query::SelectStatement {
        use sea_orm::QueryTrait;
        runner_entity::Entity::find_by_id(runner_id).into_query()
    }
}

#[cfg(test)]
mod tests {
    use super::super::models::runner;
    use super::super::test_support as ts;
    use super::*;

    fn fixture_rows() -> serde_json::Value {
        ts::fixture_file("V1CLIAUTH-F3.lookup.rows.json")
    }

    fn fixture_sql() -> String {
        ts::fixture_text("V1CLIAUTH-F3.lookup.sql")
    }

    #[test]
    fn f3_django_sql_shape_matches_fixture() {
        let raw = fixture_sql();
        // The header comments describe the shape ("no deleted filter");
        // assert against the statement text only.
        let sql: String = raw
            .lines()
            .filter(|l| !l.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n");
        // The recorded Django Q1 projects all 31 F2 columns quoted.
        for col in runner::COLUMNS {
            let quoted = format!("\"runner\".\"{col}\"");
            assert!(sql.contains(&quoted), "Django Q1 projects {quoted}");
        }
        assert!(
            sql.contains("WHERE \"runner\".\"id\" = %(runner_id)s"),
            "Django Q1 filters on pk"
        );
        assert!(
            sql.contains("ORDER BY \"runner\".\"last_heartbeat_at\" DESC"),
            "Django Q1 carries Meta.ordering"
        );
        assert!(sql.contains("LIMIT 1"), "Django Q1 carries .first() limit");
        // Plain lookup: no soft-delete filter (no such column), no join.
        assert!(
            !sql.to_lowercase().contains("deleted"),
            "no deleted filter anywhere in the F3 SQL"
        );
        assert!(!sql.to_lowercase().contains("join"), "no join on this path");
        // The port-shape note pins the plain form.
        assert!(
            raw.contains("SELECT <31 cols> FROM runner WHERE id = $1"),
            "port-shape note"
        );
    }

    #[test]
    fn f3_port_stmt_projects_31_cols_with_plain_pk_filter() {
        use sea_query::PostgresQueryBuilder;
        let sql = runner_lookup::by_pk_stmt(
            uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
        )
        .to_string(PostgresQueryBuilder);
        // Same projected set as the Django Q1 / F2 columns (order free:
        // the entity emits its declaration order).
        for col in runner::COLUMNS {
            let quoted = format!("\"runner\".\"{col}\"");
            assert!(sql.contains(&quoted), "by-pk stmt projects {quoted}");
        }
        assert!(sql.contains("FROM \"runner\""), "lookup table");
        assert!(
            sql.contains("WHERE \"runner\".\"id\" = '22222222-2222-2222-2222-222222222222'"),
            "plain pk filter"
        );
        // Vacuous ORDER BY + LIMIT 1 omitted (the pk lookup is already
        // single-row); still no deleted filter, no join.
        assert!(!sql.contains("ORDER BY"), "no vacuous ordering");
        assert!(!sql.contains("LIMIT"), "no limit clause");
        assert!(!sql.to_lowercase().contains("deleted"), "no deleted filter");
        assert!(!sql.to_lowercase().contains("join"), "no join");
    }

    /// Live-db test over a session-local copy of `runner`: the by-pk
    /// lookup reads the seeded row back and misses unknown ids. Skips
    /// without `DATABASE_URL`.
    #[tokio::test]
    async fn live_find_by_pk_reads_shadow_row() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        let migrated: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('public.runner')::text")
                .fetch_one(&pool)
                .await
                .expect("probe");
        if migrated.is_none() {
            eprintln!("skipping live-db test: the database is not migrated");
            return;
        }
        // Session-scoped twin (the one-connection pool keeps it on the
        // same session); no ON COMMIT DROP — there is no explicit
        // transaction, so the table would drop at the first commit.
        sqlx::query("CREATE TEMP TABLE runner AS SELECT * FROM public.runner WITH NO DATA")
            .execute(&pool)
            .await
            .expect("temp table");
        let id = uuid::Uuid::new_v4();
        let owner_id = uuid::Uuid::new_v4();
        let workspace_id = uuid::Uuid::new_v4();
        let pod_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now();
        sqlx::query(
            "INSERT INTO runner (id, owner_id, workspace_id, pod_id, name,
                 host_label, provisioning, visibility, refresh_token_hash,
                 refresh_token_fingerprint, refresh_token_generation,
                 previous_refresh_token_hash, access_token_signing_key_version,
                 enrollment_token_hash, enrollment_token_fingerprint,
                 capabilities, status, os, arch, runner_version,
                 dev_metadata, protocol_version, created_at, updated_at,
                 revoked_reason)
             VALUES ($1, $2, $3, $4, 'live-runner', '', 'manual', 0, '', '',
                     0, '', 1, '', '', '[]', 'offline', '', '', '',
                     '{}', 1, $5, $5, '')",
        )
        .bind(id)
        .bind(owner_id)
        .bind(workspace_id)
        .bind(pod_id)
        .bind(now)
        .execute(&pool)
        .await
        .expect("seed row");
        let hit = runner_lookup::find_by_pk(&pool, id)
            .await
            .expect("lookup")
            .expect("hit");
        assert_eq!(hit.id, id);
        assert_eq!(hit.owner_id, owner_id);
        assert_eq!(hit.name, "live-runner");
        assert_eq!(hit.visibility, 0);
        assert_eq!(hit.status, "offline");
        assert!(hit.capabilities.is_array());
        assert!(hit.dev_metadata.is_object());
        assert!(hit.revoked_at.is_none());
        let miss = runner_lookup::find_by_pk(&pool, uuid::Uuid::new_v4())
            .await
            .expect("lookup");
        assert!(miss.is_none());
    }

    #[test]
    fn f3_example_row_keys_match_columns_and_deserialize() {
        let rows = fixture_rows();
        assert_eq!(rows["fixture"].as_str(), Some("V1CLIAUTH-F3"));
        let hit = &rows["example_rows"]["q1_by_pk_hit"];
        let mut keys: Vec<&str> = hit
            .as_object()
            .expect("q1_by_pk_hit is an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut cols: Vec<&str> = runner::COLUMNS.to_vec();
        cols.sort_unstable();
        assert_eq!(keys, cols, "example row carries exactly the 31 columns");
        // The example row deserializes into the read model: types line up
        // (UUIDs, i16/i32 ints, timestamptz datetimes, JSON values).
        let runner: Runner = serde_json::from_value(hit.clone()).expect("example row fits Runner");
        assert_eq!(runner.visibility, 0);
        assert_eq!(runner.access_token_signing_key_version, 1);
        assert_eq!(runner.protocol_version, 1);
        assert_eq!(runner.refresh_token_generation, 0);
        assert!(runner.capabilities.is_array());
        assert!(runner.dev_metadata.is_object());
        // Guard-facts projection off the row: (visibility, owner_id),
        // no second query.
        let facts = runner_lookup::guard_facts(&runner);
        assert_eq!(
            facts.owner_id.to_string(),
            "22222222-2222-2222-2222-222222222222"
        );
        assert_eq!(facts.visibility, 0);
        let guard_doc = &rows["example_rows"]["guard_facts_owner_sees"];
        assert_eq!(
            guard_doc["visibility"].as_i64(),
            Some(facts.visibility as i64)
        );
        assert_eq!(
            guard_doc["owner_id"].as_str(),
            Some("22222222-2222-2222-2222-222222222222")
        );
    }

    #[test]
    fn f3_miss_and_guard_projection_notes_match_fixture() {
        let rows = fixture_rows();
        assert_eq!(
            rows["queries"]["q2_guard_facts"].as_str(),
            Some("attribute reads (visibility, owner_id) off the Q1 row (permissions.py:74-75, :135-136); no SQL")
        );
        assert!(
            rows["example_rows"]["q1_by_pk_miss"]
                .as_str()
                .expect("miss note is a string")
                .contains("404"),
            "unknown id maps to the handler 404"
        );
        assert_eq!(runner_lookup::TABLE, runner::TABLE);
    }
}
