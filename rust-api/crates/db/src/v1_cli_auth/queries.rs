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
//! SQL execution uses runtime `sqlx::query` (no `query!` macros): there
//! is no build-time database and no `.sqlx` offline cache, following
//! the merged `license/queries` precedent. Django's `%s` placeholders
//! render as Postgres `$N` here; the projected column set and order
//! match the recorded Django SQL exactly.
//!
//! # Port shape vs Django shape (same semantics)
//!
//! Django's Q1 carries `ORDER BY "runner"."last_heartbeat_at" DESC,
//! "runner"."created_at" DESC LIMIT 1` (`Meta.ordering` + `.first()`).
//! The ordering is semantically vacuous on a pk lookup (at most one row
//! matches), so the port uses the plain shape — `SELECT <30 cols> FROM
//! runner WHERE id = $1` — and [`runner_lookup::find_by_pk`] uses
//! `fetch_optional` for the `.first()` LIMIT-1 cardinality.
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * None observed on this path (fixture TRACE "Ported bugs"). The 403
//!   branch (`api/views/runner.py:50-51`) and the `:137`
//!   owner-or-admin line are unreachable-but-by-design and stay ported;
//!   they live in the handler layer (PIDASHCONV-538), not here.

use sqlx::postgres::PgRow;
use sqlx::Row;

use super::models::runner::Runner;

/// By-pk lookup + guard-facts projection (`views/runner.py:45-51`).
pub mod runner_lookup {
    use super::*;

    /// Django table name (`Meta.db_table`, `runner/models.py:479`).
    pub const TABLE: &str = "runner";

    /// `Runner.objects.filter(pk=runner_id).first()` projected to the
    /// full 30-column row: the guard reads (`visibility`, `owner_id`)
    /// come off it and `delete_runner` takes the row itself, so there
    /// is no narrow projection. Fixture F3 `q1_by_pk` (Django `%s` →
    /// `$1`); the vacuous `ORDER BY` + `LIMIT 1` are omitted (see
    /// module docs).
    pub const BY_PK_SQL: &str = "SELECT \"runner\".\"id\", \"runner\".\"owner_id\", \"runner\".\"workspace_id\", \"runner\".\"dev_machine_id\", \"runner\".\"pod_id\", \"runner\".\"name\", \"runner\".\"host_label\", \"runner\".\"provisioning\", \"runner\".\"visibility\", \"runner\".\"refresh_token_hash\", \"runner\".\"refresh_token_fingerprint\", \"runner\".\"refresh_token_generation\", \"runner\".\"previous_refresh_token_hash\", \"runner\".\"access_token_signing_key_version\", \"runner\".\"enrollment_token_hash\", \"runner\".\"enrollment_token_fingerprint\", \"runner\".\"enrolled_at\", \"runner\".\"capabilities\", \"runner\".\"status\", \"runner\".\"os\", \"runner\".\"arch\", \"runner\".\"runner_version\", \"runner\".\"dev_metadata\", \"runner\".\"protocol_version\", \"runner\".\"last_heartbeat_at\", \"runner\".\"free_worktrees\", \"runner\".\"created_at\", \"runner\".\"updated_at\", \"runner\".\"revoked_at\", \"runner\".\"revoked_reason\" FROM \"runner\" WHERE \"runner\".\"id\" = $1";

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

    /// Map a Q1 row into [`Runner`], one `try_get` per column in
    /// `COLUMNS` order (manual mapping follows the `auth_oauth`
    /// precedent; there is no `FromRow` derive in this crate).
    pub fn runner_from_row(row: &PgRow) -> Result<Runner, sqlx::Error> {
        Ok(Runner {
            id: row.try_get("id")?,
            owner_id: row.try_get("owner_id")?,
            workspace_id: row.try_get("workspace_id")?,
            dev_machine_id: row.try_get("dev_machine_id")?,
            pod_id: row.try_get("pod_id")?,
            name: row.try_get("name")?,
            host_label: row.try_get("host_label")?,
            provisioning: row.try_get("provisioning")?,
            visibility: row.try_get("visibility")?,
            refresh_token_hash: row.try_get("refresh_token_hash")?,
            refresh_token_fingerprint: row.try_get("refresh_token_fingerprint")?,
            refresh_token_generation: row.try_get("refresh_token_generation")?,
            previous_refresh_token_hash: row.try_get("previous_refresh_token_hash")?,
            access_token_signing_key_version: row.try_get("access_token_signing_key_version")?,
            enrollment_token_hash: row.try_get("enrollment_token_hash")?,
            enrollment_token_fingerprint: row.try_get("enrollment_token_fingerprint")?,
            enrolled_at: row.try_get("enrolled_at")?,
            capabilities: row.try_get("capabilities")?,
            status: row.try_get("status")?,
            os: row.try_get("os")?,
            arch: row.try_get("arch")?,
            runner_version: row.try_get("runner_version")?,
            dev_metadata: row.try_get("dev_metadata")?,
            protocol_version: row.try_get("protocol_version")?,
            last_heartbeat_at: row.try_get("last_heartbeat_at")?,
            free_worktrees: row.try_get("free_worktrees")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            revoked_at: row.try_get("revoked_at")?,
            revoked_reason: row.try_get("revoked_reason")?,
        })
    }

    /// `Runner.objects.filter(pk=runner_id).first()`
    /// (`views/runner.py:45`): `None` when the id is unknown (handler
    /// 404). Callers pick the pool and pass an executor in; every
    /// executor is generic over `sqlx::Executor`.
    pub async fn find_by_pk<'e, E>(
        ex: E,
        runner_id: uuid::Uuid,
    ) -> Result<Option<Runner>, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row: Option<PgRow> = sqlx::query(BY_PK_SQL)
            .bind(runner_id)
            .fetch_optional(ex)
            .await?;
        row.map(|r| runner_from_row(&r)).transpose()
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
        // The recorded Django Q1 projects all 30 F2 columns quoted.
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
            raw.contains("SELECT <30 cols> FROM runner WHERE id = $1"),
            "port-shape note"
        );
    }

    #[test]
    fn f3_port_sql_projects_30_cols_with_plain_pk_filter() {
        let sql = runner_lookup::BY_PK_SQL;
        // Same projected set and order as the Django Q1 / F2 columns.
        let mut cursor = 0;
        for col in runner::COLUMNS {
            let quoted = format!("\"runner\".\"{col}\"");
            let pos = sql[cursor..]
                .find(&quoted)
                .unwrap_or_else(|| panic!("BY_PK_SQL projects {quoted}"));
            cursor += pos + quoted.len();
        }
        assert!(sql.contains("FROM \"runner\""), "lookup table");
        assert!(
            sql.contains("WHERE \"runner\".\"id\" = $1"),
            "plain pk filter, Django %s rendered as $1"
        );
        // Vacuous ORDER BY + LIMIT 1 omitted (fetch_optional provides
        // the .first() cardinality); still no deleted filter, no join.
        assert!(!sql.contains("ORDER BY"), "no vacuous ordering");
        assert!(!sql.contains("LIMIT"), "no limit clause");
        assert!(!sql.to_lowercase().contains("deleted"), "no deleted filter");
        assert!(!sql.to_lowercase().contains("join"), "no join");
        // Exactly one bind parameter.
        assert_eq!(sql.matches("$1").count(), 1, "single bind param");
        assert!(!sql.contains("$2"), "no second bind param");
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
        assert_eq!(keys, cols, "example row carries exactly the 30 columns");
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
