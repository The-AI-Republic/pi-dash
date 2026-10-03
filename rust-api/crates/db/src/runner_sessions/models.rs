#![forbid(unsafe_code)]

//! Session row models: [`runner_session::RunnerSession`] +
//! [`machine_session::MachineSession`].
//!
//! One submodule per model (mirroring the one-file-per-model layout
//! of `runner_runs`, kept in a single file per this issue's named
//! paths). Each submodule carries its table/column/constraint/index
//! consts, its row struct with manual `try_get` mapping, and the
//! seven session-row SQL statements this domain executes against
//! that table. Django-shaped SQL throughout (quoted identifiers,
//! `%s` params rendered as Postgres `$N`, `$N` assigned left to
//! right in `WHERE`-term order); Django stays schema owner, so
//! there is no DDL here.
//!
//! SQL provenance: every const below was captured from the real ORM
//! call (Django 4.2.30, scratch Postgres) and the active-id pair
//! additionally replays the FX-RSES-05 bytes. Capture notes:
//!
//! * `SELECT` statements via `str(queryset.query)` inside an
//!   (uncommitted, rolled-back) transaction: open's
//!   `select_for_update().filter(...).first()` renders the full
//!   projection + `WHERE` + `ORDER BY "created_at" DESC LIMIT 1 FOR
//!   UPDATE`; `.get()` clears `Meta.ordering` and applies
//!   `LIMIT 21` (`MAX_GET_RESULTS`, verified `== 21`); the
//!   active-id `values_list("id").first()` renders the id
//!   projection + `ORDER BY "created_at" DESC LIMIT 1`.
//! * `INSERT` / `UPDATE` via `CaptureQueriesContext` (the debug
//!   cursor logs each statement in a `finally`, so the exact text is
//!   recorded even though the scratch tables do not exist).
//!   `create()` lists all seven columns in field-definition order
//!   with no `RETURNING` (the PK is caller-supplied);
//!   `save(update_fields=[...])` on a loaded row (`_state.adding =
//!   False`) renders `SET` in `update_fields` order + `WHERE pk`.
//! * `WHERE` AND-term order is alphabetical by column in all eight
//!   captured selects (the `runner_id` term sorts last, the
//!   `dev_machine_id` term first) — encoded verbatim, not
//!   re-derived. The active-id captures reproduce the FX-RSES-05
//!   bytes exactly, which validates the capture setup end to end.

/// Per-runner cloud session (`runner/models.py:688-720`).
///
/// Owns delivery for one runner: the row is created on
/// `POST /runners/<rid>/sessions/` and revoked on session-eviction,
/// idle-timeout, or runner revocation. Exactly one active session per
/// runner is enforced by [`runner_session::ONE_ACTIVE_PER_RUNNER`].
pub mod runner_session {
    use serde::{Deserialize, Serialize};

    use crate::integrations::OnDelete;

    /// Physical table (`Meta.db_table`, `models.py:708`).
    pub const TABLE: &str = "runner_session";
    /// Default ordering (`Meta.ordering`, `models.py:709`).
    pub const ORDERING: &[&str] = &["-created_at"];

    /// Columns in declaration order (`models.py:699-705`), FK entry
    /// as the physical `runner_id` column.
    pub const COLUMNS: &[&str] = &[
        "id",
        "runner_id",
        "protocol_version",
        "created_at",
        "last_seen_at",
        "revoked_at",
        "revoked_reason",
    ];

    /// `runner` FK target table (`models.py:700`).
    pub const FK_TARGET_TABLE: &str = "runner";
    /// `runner` FK physical column (`models.py:700`).
    pub const FK_COLUMN: &str = "runner_id";
    /// `runner` FK delete rule: `CASCADE` (`models.py:700`).
    pub const FK_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `runner` FK reverse accessor (`models.py:700`).
    pub const FK_RELATED_NAME: &str = "sessions";

    /// `protocol_version` Django-side default (`models.py:701`,
    /// `default=4`). Session open passes
    /// `settings.RUNNER_PROTOCOL_VERSION` explicitly (verified `== 4`
    /// at capture time); the value coincides with the field default
    /// today.
    pub const DEFAULT_PROTOCOL_VERSION: i32 = 4;
    /// `revoked_reason` Django-side default (`models.py:705`,
    /// `default=""`).
    pub const DEFAULT_REVOKED_REASON: &str = "";
    /// `revoked_reason` bound (`models.py:705`, `max_length=32`).
    pub const REVOKED_REASON_MAX_LENGTH: usize = 32;

    /// Exactly one active session per runner (`models.py:710-716`):
    /// `runner` is unique while `revoked_at IS NULL`.
    pub const ONE_ACTIVE_PER_RUNNER: &str = "runner_session_one_active_per_runner";
    /// Fields of [`ONE_ACTIVE_PER_RUNNER`], Django field names.
    pub const ONE_ACTIVE_PER_RUNNER_FIELDS: &[&str] = &["runner"];
    /// SQL form of the [`ONE_ACTIVE_PER_RUNNER`] condition
    /// (`models.py:713`, `Q(revoked_at__isnull=True)`).
    pub const ONE_ACTIVE_PER_RUNNER_CONDITION_SQL: &str =
        "\"runner_session\".\"revoked_at\" IS NULL";

    /// Runner + revocation lookup index (`models.py:718`).
    pub const RUNNER_REVOKED_INDEX: &str = "runner_sess_runner__f0ef41_idx";
    /// Fields of [`RUNNER_REVOKED_INDEX`], Django field names.
    pub const RUNNER_REVOKED_INDEX_FIELDS: &[&str] = &["runner", "revoked_at"];
    /// Last-seen sweep index (`models.py:719`).
    pub const LAST_SEEN_INDEX: &str = "runner_sess_last_se_2406f1_idx";
    /// Fields of [`LAST_SEEN_INDEX`], Django field names.
    pub const LAST_SEEN_INDEX_FIELDS: &[&str] = &["last_seen_at"];

    /// Session-open prior lookup (`views/sessions.py:174-177`):
    /// `select_for_update().filter(runner=runner,
    /// revoked_at__isnull=True).first()`. `$1` is the runner id.
    /// The `IS NULL` term precedes the `runner_id` term (captured
    /// order); the row lock is held for the open transaction's
    /// lifetime, which is why Redis I/O stays outside the
    /// `atomic()` block (`:156-167`).
    pub const OPEN_PRIOR_SQL: &str = "SELECT \"runner_session\".\"id\", \"runner_session\".\"runner_id\", \"runner_session\".\"protocol_version\", \"runner_session\".\"created_at\", \"runner_session\".\"last_seen_at\", \"runner_session\".\"revoked_at\", \"runner_session\".\"revoked_reason\" FROM \"runner_session\" WHERE (\"runner_session\".\"revoked_at\" IS NULL AND \"runner_session\".\"runner_id\" = $1) ORDER BY \"runner_session\".\"created_at\" DESC LIMIT 1 FOR UPDATE";
    /// Session-open insert (`views/sessions.py:184-189`):
    /// `create(id=new_sid, runner=runner,
    /// protocol_version=RUNNER_PROTOCOL_VERSION,
    /// last_seen_at=now)`. Params in column order: `$1` id (fresh
    /// `uuid4`, caller-supplied), `$2` runner id, `$3` protocol
    /// version, `$4` `created_at` (`auto_now_add`, Python-side
    /// `now`), `$5` `last_seen_at`, `$6` `revoked_at` (`NULL`: no
    /// default, nullable), `$7` `revoked_reason` (`""` default). No
    /// `RETURNING`: the PK is supplied, so Django fetches nothing
    /// back.
    pub const OPEN_INSERT_SQL: &str = "INSERT INTO \"runner_session\" (\"id\", \"runner_id\", \"protocol_version\", \"created_at\", \"last_seen_at\", \"revoked_at\", \"revoked_reason\") VALUES ($1, $2, $3, $4, $5, $6, $7)";
    /// Session revoke (`views/sessions.py:180-182` open-eviction with
    /// reason `"evicted_by_new_session"`, `:307-309` delete with
    /// `"clean_shutdown"`): `save(update_fields=["revoked_at",
    /// "revoked_reason"])`. `$1` revoked_at, `$2` revoked_reason,
    /// `$3` session id. `close_runner_session` (`pubsub.py:101-104`,
    /// reason `"force_close"`, PIDASHCONV-553) emits this same shape.
    pub const REVOKE_SQL: &str = "UPDATE \"runner_session\" SET \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE \"runner_session\".\"id\" = $3";
    /// Session-delete lookup (`views/sessions.py:303`):
    /// `get(id=sid, runner=runner, revoked_at__isnull=True)`; miss
    /// (or an already-revoked row) still clears the marker and
    /// returns 204 (`:304-306`), so callers treat "no row" as
    /// success. `$1` session id, `$2` runner id. `.get()` clears
    /// `Meta.ordering` and caps at `LIMIT 21`.
    pub const DELETE_GET_SQL: &str = "SELECT \"runner_session\".\"id\", \"runner_session\".\"runner_id\", \"runner_session\".\"protocol_version\", \"runner_session\".\"created_at\", \"runner_session\".\"last_seen_at\", \"runner_session\".\"revoked_at\", \"runner_session\".\"revoked_reason\" FROM \"runner_session\" WHERE (\"runner_session\".\"id\" = $1 AND \"runner_session\".\"revoked_at\" IS NULL AND \"runner_session\".\"runner_id\" = $2) LIMIT 21";
    /// Poll-bookkeeping lookup (`views/sessions.py:339`):
    /// `get(id=sid, runner=runner)` — note: no `revoked_at` filter;
    /// a revoked row is fetched and rejected with its reason
    /// (`:345-352`). `$1` session id, `$2` runner id. Ordering
    /// cleared, `LIMIT 21`, like every `.get()`.
    pub const POLL_GET_SQL: &str = "SELECT \"runner_session\".\"id\", \"runner_session\".\"runner_id\", \"runner_session\".\"protocol_version\", \"runner_session\".\"created_at\", \"runner_session\".\"last_seen_at\", \"runner_session\".\"revoked_at\", \"runner_session\".\"revoked_reason\" FROM \"runner_session\" WHERE (\"runner_session\".\"id\" = $1 AND \"runner_session\".\"runner_id\" = $2) LIMIT 21";
    /// Poll-bookkeeping touch (`views/sessions.py:359-360`):
    /// `save(update_fields=["last_seen_at"])`. `$1` last_seen_at,
    /// `$2` session id.
    pub const TOUCH_SQL: &str =
        "UPDATE \"runner_session\" SET \"last_seen_at\" = $1 WHERE \"runner_session\".\"id\" = $2";
    /// Active-session id lookup
    /// (`services/outbox.py:207-213`): `filter(runner_id=runner_id,
    /// revoked_at__isnull=True).values_list("id",
    /// flat=True).first()`; `None` when no active row. `$1` is the
    /// runner id. Replays FX-RSES-05 `active_session_id_for_runner`.
    pub const ACTIVE_ID_SQL: &str = "SELECT \"runner_session\".\"id\" FROM \"runner_session\" WHERE (\"runner_session\".\"revoked_at\" IS NULL AND \"runner_session\".\"runner_id\" = $1) ORDER BY \"runner_session\".\"created_at\" DESC LIMIT 1";

    /// One runner-session row, declaration order. `protocol_version`
    /// is a `PositiveIntegerField` (`models.py:701`), hence `i32`
    /// like the D-02 `repository_count` port. `id` has no
    /// Rust-side default: both writes supply it (open passes a fresh
    /// `uuid4`, `views/sessions.py:169+184`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct RunnerSession {
        pub id: uuid::Uuid,
        pub runner_id: uuid::Uuid,
        pub protocol_version: i32,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
        pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
        pub revoked_reason: String,
    }

    /// Map an open/delete/poll row into [`RunnerSession`], one
    /// `try_get` per column in [`COLUMNS`] order (manual mapping
    /// follows the `v1_cli_auth` precedent; there is no `FromRow`
    /// derive in this crate).
    pub fn runner_session_from_row(
        row: &sqlx::postgres::PgRow,
    ) -> Result<RunnerSession, sqlx::Error> {
        use sqlx::Row;
        Ok(RunnerSession {
            id: row.try_get("id")?,
            runner_id: row.try_get("runner_id")?,
            protocol_version: row.try_get("protocol_version")?,
            created_at: row.try_get("created_at")?,
            last_seen_at: row.try_get("last_seen_at")?,
            revoked_at: row.try_get("revoked_at")?,
            revoked_reason: row.try_get("revoked_reason")?,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::runner_sessions::test_support as ts;

        fn session_fixture() -> serde_json::Value {
            ts::owned(&ts::fx01(), "RunnerSession").clone()
        }

        /// Assert every `needle` appears in `sql` left to right, in order.
        fn assert_terms_in_order(sql: &str, needles: &[&str]) {
            let mut cursor = 0;
            for needle in needles {
                let pos = sql[cursor..]
                    .find(needle)
                    .unwrap_or_else(|| panic!("SQL carries {needle}: {sql}"));
                cursor += pos + needle.len();
            }
        }

        /// Assert the full-row projection lists every [`COLUMNS`] entry in order.
        fn assert_full_projection(sql: &str) {
            let mut cursor = 0;
            for col in COLUMNS {
                let quoted = format!("\"runner_session\".\"{col}\"");
                let pos = sql[cursor..]
                    .find(quoted.as_str())
                    .unwrap_or_else(|| panic!("projection carries {quoted}: {sql}"));
                cursor += pos + quoted.len();
            }
        }

        #[test]
        fn fixture_identity() {
            assert_eq!(ts::fx01()["fixture"].as_str(), Some("FX-RSES-01"));
            assert_eq!(ts::fx05()["fixture"].as_str(), Some("FX-RSES-05"));
        }

        #[test]
        fn columns_match_fixture_in_order() {
            assert_eq!(ts::owned_cols(COLUMNS), ts::columns(&session_fixture()));
            assert_eq!(COLUMNS.len(), 7);
        }

        #[test]
        fn meta_matches_fixture() {
            let m = session_fixture();
            assert_eq!(TABLE, m["db_table"].as_str().expect("db_table"));
            assert_eq!(serde_json::json!(ORDERING), m["ordering"], "Meta.ordering");
            assert!(m["unique_together"].as_array().expect("ut").is_empty());
        }

        #[test]
        fn constraint_matches_fixture() {
            let m = session_fixture();
            let uniq = ts::constraint(&m, ONE_ACTIVE_PER_RUNNER);
            assert_eq!(uniq["type"].as_str(), Some("UniqueConstraint"));
            assert_eq!(
                uniq["fields"],
                serde_json::json!(ONE_ACTIVE_PER_RUNNER_FIELDS)
            );
            assert_eq!(
                uniq["condition"].as_str(),
                Some("(AND: ('revoked_at__isnull', True))")
            );
            assert_eq!(
                ONE_ACTIVE_PER_RUNNER_CONDITION_SQL,
                "\"runner_session\".\"revoked_at\" IS NULL"
            );
        }

        #[test]
        fn indexes_match_fixture() {
            let m = session_fixture();
            let pair = ts::index(&m, RUNNER_REVOKED_INDEX);
            assert_eq!(
                pair["fields"],
                serde_json::json!(RUNNER_REVOKED_INDEX_FIELDS)
            );
            let seen = ts::index(&m, LAST_SEEN_INDEX);
            assert_eq!(seen["fields"], serde_json::json!(LAST_SEEN_INDEX_FIELDS));
        }

        #[test]
        fn defaults_types_and_relations_match_fixture() {
            let m = session_fixture();
            let id = ts::field(&m, "id");
            assert_eq!(id["type"].as_str(), Some("UUIDField"));
            assert_eq!(id["primary_key"].as_bool(), Some(true));
            assert_eq!(id["unique"].as_bool(), Some(true));
            assert_eq!(id["default"].as_str(), Some("<callable uuid4>"));
            let runner = ts::field(&m, "runner");
            assert_eq!(runner["type"].as_str(), Some("ForeignKey"));
            assert_eq!(runner["column"].as_str(), Some(FK_COLUMN));
            assert_eq!(runner["null"].as_bool(), Some(false));
            assert_eq!(runner["related_model"].as_str(), Some("runner.Runner"));
            assert_eq!(runner["related_name"].as_str(), Some(FK_RELATED_NAME));
            assert_eq!(runner["on_delete"].as_str(), Some("CASCADE"));
            assert_eq!(FK_ON_DELETE, OnDelete::Cascade);
            assert_eq!(FK_TARGET_TABLE, "runner");
            let proto = ts::field(&m, "protocol_version");
            assert_eq!(proto["type"].as_str(), Some("PositiveIntegerField"));
            assert_eq!(proto["null"].as_bool(), Some(false));
            assert_eq!(proto["default"].as_str(), Some("4"));
            assert_eq!(DEFAULT_PROTOCOL_VERSION, 4);
            let created = ts::field(&m, "created_at");
            assert_eq!(created["type"].as_str(), Some("DateTimeField"));
            assert_eq!(created["null"].as_bool(), Some(false));
            for name in ["last_seen_at", "revoked_at"] {
                let f = ts::field(&m, name);
                assert_eq!(f["type"].as_str(), Some("DateTimeField"), "{name} type");
                assert_eq!(f["null"].as_bool(), Some(true), "{name} null");
            }
            let reason = ts::field(&m, "revoked_reason");
            assert_eq!(reason["type"].as_str(), Some("CharField"));
            assert_eq!(reason["null"].as_bool(), Some(false));
            assert_eq!(reason["max_length"].as_u64(), Some(32));
            assert_eq!(reason["default"].as_str(), Some("''"));
            assert_eq!(REVOKED_REASON_MAX_LENGTH, 32);
            assert_eq!(DEFAULT_REVOKED_REASON, "");
        }

        #[test]
        fn active_id_sql_replays_fixture() {
            let section = &ts::fx05()["active_session_id_for_runner"];
            for side in ["none", "some"] {
                let django = section[side]["sql"][0].as_str().expect("sql");
                assert_eq!(ts::dollarize_param(django), ACTIVE_ID_SQL, "{side} SQL");
            }
            assert!(section["none"]["value"].is_null());
            section["some"]["value"]
                .as_str()
                .expect("uuid")
                .parse::<uuid::Uuid>()
                .expect("some value parses as a UUID");
            assert_eq!(section["some"]["matches"].as_bool(), Some(true));
        }

        #[test]
        fn open_prior_sql_matches_captured_django() {
            let sql = OPEN_PRIOR_SQL;
            assert_full_projection(sql);
            // Byte pin of the WHERE term order (IS NULL term first, captured).
            assert_terms_in_order(
                sql,
                &[
                    "FROM \"runner_session\" WHERE (\"runner_session\".\"revoked_at\" IS NULL AND \"runner_session\".\"runner_id\" = $1)",
                    "ORDER BY \"runner_session\".\"created_at\" DESC",
                    "LIMIT 1 FOR UPDATE",
                ],
            );
            assert!(sql.ends_with("LIMIT 1 FOR UPDATE"));
        }

        #[test]
        fn open_insert_sql_matches_captured_django() {
            let sql = OPEN_INSERT_SQL;
            assert_terms_in_order(
                sql,
                &[
                    "INSERT INTO \"runner_session\" (\"id\", \"runner_id\", \"protocol_version\", \"created_at\", \"last_seen_at\", \"revoked_at\", \"revoked_reason\")",
                    "VALUES ($1, $2, $3, $4, $5, $6, $7)",
                ],
            );
            assert!(
                !sql.contains("RETURNING"),
                "no RETURNING: the PK is caller-supplied"
            );
        }

        #[test]
        fn revoke_sql_matches_captured_django() {
            // SET order is update_fields order; the WHERE is the bare pk.
            assert_eq!(
                REVOKE_SQL,
                "UPDATE \"runner_session\" SET \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE \"runner_session\".\"id\" = $3"
            );
        }

        #[test]
        fn delete_get_sql_matches_captured_django() {
            let sql = DELETE_GET_SQL;
            assert_full_projection(sql);
            assert_terms_in_order(
                sql,
                &[
                    "FROM \"runner_session\" WHERE (\"runner_session\".\"id\" = $1 AND \"runner_session\".\"revoked_at\" IS NULL AND \"runner_session\".\"runner_id\" = $2)",
                    "LIMIT 21",
                ],
            );
            assert!(!sql.contains("ORDER BY"), ".get() clears Meta.ordering");
            assert!(sql.ends_with("LIMIT 21"));
        }

        #[test]
        fn poll_get_sql_matches_captured_django() {
            let sql = POLL_GET_SQL;
            assert_full_projection(sql);
            // No revoked_at term: revoked rows are fetched, then rejected
            // with their reason (views/sessions.py:345-352).
            assert_terms_in_order(
                sql,
                &[
                    "FROM \"runner_session\" WHERE (\"runner_session\".\"id\" = $1 AND \"runner_session\".\"runner_id\" = $2)",
                    "LIMIT 21",
                ],
            );
            assert!(!sql.contains("ORDER BY"), ".get() clears Meta.ordering");
            assert!(sql.ends_with("LIMIT 21"));
        }

        #[test]
        fn touch_sql_matches_captured_django() {
            assert_eq!(
                TOUCH_SQL,
                "UPDATE \"runner_session\" SET \"last_seen_at\" = $1 WHERE \"runner_session\".\"id\" = $2"
            );
        }
    }
}

/// Machine-level control session (`runner/models.py:723-762`).
///
/// The twin of [`runner_session::RunnerSession`], scoped to a whole
/// `DevMachine` rather than a single runner: the daemon opens exactly
/// one on startup and long-polls it for machine-scoped control
/// messages. Exactly one active session per machine is enforced by
/// [`machine_session::ONE_ACTIVE_PER_MACHINE`].
pub mod machine_session {
    use serde::{Deserialize, Serialize};

    use crate::integrations::OnDelete;

    /// Physical table (`Meta.db_table`, `models.py:750`).
    pub const TABLE: &str = "machine_session";
    /// Default ordering (`Meta.ordering`, `models.py:751`).
    pub const ORDERING: &[&str] = &["-created_at"];

    /// Columns in declaration order (`models.py:739-747`), FK entry
    /// as the physical `dev_machine_id` column.
    pub const COLUMNS: &[&str] = &[
        "id",
        "dev_machine_id",
        "protocol_version",
        "created_at",
        "last_seen_at",
        "revoked_at",
        "revoked_reason",
    ];

    /// `dev_machine` FK target table (`models.py:740-742`).
    pub const FK_TARGET_TABLE: &str = "dev_machine";
    /// `dev_machine` FK physical column (`models.py:740-742`).
    pub const FK_COLUMN: &str = "dev_machine_id";
    /// `dev_machine` FK delete rule: `CASCADE` (`models.py:740-742`).
    pub const FK_ON_DELETE: OnDelete = OnDelete::Cascade;
    /// `dev_machine` FK reverse accessor (`models.py:740-742`).
    pub const FK_RELATED_NAME: &str = "sessions";

    /// `protocol_version` Django-side default (`models.py:743`,
    /// `default=4`). Machine open passes
    /// `settings.RUNNER_PROTOCOL_VERSION` explicitly (verified `== 4`
    /// at capture time); the value coincides with the field default
    /// today.
    pub const DEFAULT_PROTOCOL_VERSION: i32 = 4;
    /// `revoked_reason` Django-side default (`models.py:747`,
    /// `default=""`).
    pub const DEFAULT_REVOKED_REASON: &str = "";
    /// `revoked_reason` bound (`models.py:747`, `max_length=32`).
    pub const REVOKED_REASON_MAX_LENGTH: usize = 32;

    /// Exactly one active session per machine (`models.py:752-758`):
    /// `dev_machine` is unique while `revoked_at IS NULL`.
    pub const ONE_ACTIVE_PER_MACHINE: &str = "machine_session_one_active_per_machine";
    /// Fields of [`ONE_ACTIVE_PER_MACHINE`], Django field names.
    pub const ONE_ACTIVE_PER_MACHINE_FIELDS: &[&str] = &["dev_machine"];
    /// SQL form of the [`ONE_ACTIVE_PER_MACHINE`] condition
    /// (`models.py:755`, `Q(revoked_at__isnull=True)`).
    pub const ONE_ACTIVE_PER_MACHINE_CONDITION_SQL: &str =
        "\"machine_session\".\"revoked_at\" IS NULL";

    /// Machine + revocation lookup index (`models.py:760`).
    pub const MACHINE_REVOKED_INDEX: &str = "machine_ses_dev_mac_885f2b_idx";
    /// Fields of [`MACHINE_REVOKED_INDEX`], Django field names.
    pub const MACHINE_REVOKED_INDEX_FIELDS: &[&str] = &["dev_machine", "revoked_at"];
    /// Last-seen sweep index (`models.py:761`).
    pub const LAST_SEEN_INDEX: &str = "machine_ses_last_se_b57ca0_idx";
    /// Fields of [`LAST_SEEN_INDEX`], Django field names.
    pub const LAST_SEEN_INDEX_FIELDS: &[&str] = &["last_seen_at"];

    /// Session-open prior lookup
    /// (`views/machine_sessions.py:90-93`):
    /// `select_for_update().filter(dev_machine=machine,
    /// revoked_at__isnull=True).first()`. `$1` is the machine id.
    /// Unlike the runner twin, the `dev_machine_id` term precedes
    /// the `IS NULL` term (captured order). Machine open runs no
    /// `_bound_txn_waits` and maps no timeout to 503 — the
    /// runner-only asymmetry noted in the parent module docs.
    pub const OPEN_PRIOR_SQL: &str = "SELECT \"machine_session\".\"id\", \"machine_session\".\"dev_machine_id\", \"machine_session\".\"protocol_version\", \"machine_session\".\"created_at\", \"machine_session\".\"last_seen_at\", \"machine_session\".\"revoked_at\", \"machine_session\".\"revoked_reason\" FROM \"machine_session\" WHERE (\"machine_session\".\"dev_machine_id\" = $1 AND \"machine_session\".\"revoked_at\" IS NULL) ORDER BY \"machine_session\".\"created_at\" DESC LIMIT 1 FOR UPDATE";
    /// Session-open insert (`views/machine_sessions.py:100-105`):
    /// `create(id=new_sid, dev_machine=machine,
    /// protocol_version=RUNNER_PROTOCOL_VERSION,
    /// last_seen_at=now)`. Params in column order: `$1` id (fresh
    /// `uuid4`, caller-supplied), `$2` machine id, `$3` protocol
    /// version, `$4` `created_at` (`auto_now_add`, Python-side
    /// `now`), `$5` `last_seen_at`, `$6` `revoked_at` (`NULL`: no
    /// default, nullable), `$7` `revoked_reason` (`""` default). No
    /// `RETURNING`: the PK is supplied, so Django fetches nothing
    /// back.
    pub const OPEN_INSERT_SQL: &str = "INSERT INTO \"machine_session\" (\"id\", \"dev_machine_id\", \"protocol_version\", \"created_at\", \"last_seen_at\", \"revoked_at\", \"revoked_reason\") VALUES ($1, $2, $3, $4, $5, $6, $7)";
    /// Session revoke (`views/machine_sessions.py:96-98`
    /// open-eviction with reason `"evicted_by_new_session"`,
    /// `:155-157` delete with `"clean_shutdown"`):
    /// `save(update_fields=["revoked_at", "revoked_reason"])`. `$1`
    /// revoked_at, `$2` revoked_reason, `$3` session id.
    pub const REVOKE_SQL: &str = "UPDATE \"machine_session\" SET \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE \"machine_session\".\"id\" = $3";
    /// Session-delete lookup (`views/machine_sessions.py:149-151`):
    /// `get(id=sid, dev_machine=machine,
    /// revoked_at__isnull=True)`; miss still clears the marker and
    /// returns 204 (`:152-154`). `$1` machine id, `$2` session id
    /// (captured term order). `.get()` clears `Meta.ordering` and
    /// caps at `LIMIT 21`.
    pub const DELETE_GET_SQL: &str = "SELECT \"machine_session\".\"id\", \"machine_session\".\"dev_machine_id\", \"machine_session\".\"protocol_version\", \"machine_session\".\"created_at\", \"machine_session\".\"last_seen_at\", \"machine_session\".\"revoked_at\", \"machine_session\".\"revoked_reason\" FROM \"machine_session\" WHERE (\"machine_session\".\"dev_machine_id\" = $1 AND \"machine_session\".\"id\" = $2 AND \"machine_session\".\"revoked_at\" IS NULL) LIMIT 21";
    /// Poll-bookkeeping lookup (`views/machine_sessions.py:181`):
    /// `get(id=sid, dev_machine=machine)` — note: no `revoked_at`
    /// filter; a revoked row is fetched and rejected with its reason
    /// (`:187-194`). `$1` machine id, `$2` session id. Ordering
    /// cleared, `LIMIT 21`, like every `.get()`.
    pub const POLL_GET_SQL: &str = "SELECT \"machine_session\".\"id\", \"machine_session\".\"dev_machine_id\", \"machine_session\".\"protocol_version\", \"machine_session\".\"created_at\", \"machine_session\".\"last_seen_at\", \"machine_session\".\"revoked_at\", \"machine_session\".\"revoked_reason\" FROM \"machine_session\" WHERE (\"machine_session\".\"dev_machine_id\" = $1 AND \"machine_session\".\"id\" = $2) LIMIT 21";
    /// Poll-bookkeeping touch (`views/machine_sessions.py:199-200`):
    /// `save(update_fields=["last_seen_at"])`. `$1` last_seen_at,
    /// `$2` session id. (The sibling `DevMachine.last_seen_at`
    /// keyed update on `:201` writes the D-13 table and executes in
    /// the machine handlers, PIDASHCONV-559.)
    pub const TOUCH_SQL: &str =
        "UPDATE \"machine_session\" SET \"last_seen_at\" = $1 WHERE \"machine_session\".\"id\" = $2";
    /// Active-session id lookup
    /// (`services/machine_outbox.py:148-154`):
    /// `filter(dev_machine_id=dev_machine_id,
    /// revoked_at__isnull=True).values_list("id",
    /// flat=True).first()`; `None` when no active row. `$1` is the
    /// machine id. Replays FX-RSES-05
    /// `active_session_id_for_machine`.
    pub const ACTIVE_ID_SQL: &str = "SELECT \"machine_session\".\"id\" FROM \"machine_session\" WHERE (\"machine_session\".\"dev_machine_id\" = $1 AND \"machine_session\".\"revoked_at\" IS NULL) ORDER BY \"machine_session\".\"created_at\" DESC LIMIT 1";

    /// One machine-session row, declaration order. `protocol_version`
    /// is a `PositiveIntegerField` (`models.py:743`), hence `i32`
    /// like the D-02 `repository_count` port. `id` has no
    /// Rust-side default: both writes supply it (open passes a fresh
    /// `uuid4`, `views/machine_sessions.py:87+100`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct MachineSession {
        pub id: uuid::Uuid,
        pub dev_machine_id: uuid::Uuid,
        pub protocol_version: i32,
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
        pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
        pub revoked_reason: String,
    }

    /// Map an open/delete/poll row into [`MachineSession`], one
    /// `try_get` per column in [`COLUMNS`] order (manual mapping
    /// follows the `v1_cli_auth` precedent; there is no `FromRow`
    /// derive in this crate).
    pub fn machine_session_from_row(
        row: &sqlx::postgres::PgRow,
    ) -> Result<MachineSession, sqlx::Error> {
        use sqlx::Row;
        Ok(MachineSession {
            id: row.try_get("id")?,
            dev_machine_id: row.try_get("dev_machine_id")?,
            protocol_version: row.try_get("protocol_version")?,
            created_at: row.try_get("created_at")?,
            last_seen_at: row.try_get("last_seen_at")?,
            revoked_at: row.try_get("revoked_at")?,
            revoked_reason: row.try_get("revoked_reason")?,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::runner_sessions::test_support as ts;

        fn session_fixture() -> serde_json::Value {
            ts::owned(&ts::fx01(), "MachineSession").clone()
        }

        /// Assert every `needle` appears in `sql` left to right, in order.
        fn assert_terms_in_order(sql: &str, needles: &[&str]) {
            let mut cursor = 0;
            for needle in needles {
                let pos = sql[cursor..]
                    .find(needle)
                    .unwrap_or_else(|| panic!("SQL carries {needle}: {sql}"));
                cursor += pos + needle.len();
            }
        }

        /// Assert the full-row projection lists every [`COLUMNS`] entry in order.
        fn assert_full_projection(sql: &str) {
            let mut cursor = 0;
            for col in COLUMNS {
                let quoted = format!("\"machine_session\".\"{col}\"");
                let pos = sql[cursor..]
                    .find(quoted.as_str())
                    .unwrap_or_else(|| panic!("projection carries {quoted}: {sql}"));
                cursor += pos + quoted.len();
            }
        }

        #[test]
        fn columns_match_fixture_in_order() {
            assert_eq!(ts::owned_cols(COLUMNS), ts::columns(&session_fixture()));
            assert_eq!(COLUMNS.len(), 7);
        }

        #[test]
        fn meta_matches_fixture() {
            let m = session_fixture();
            assert_eq!(TABLE, m["db_table"].as_str().expect("db_table"));
            assert_eq!(serde_json::json!(ORDERING), m["ordering"], "Meta.ordering");
            assert!(m["unique_together"].as_array().expect("ut").is_empty());
        }

        #[test]
        fn constraint_matches_fixture() {
            let m = session_fixture();
            let uniq = ts::constraint(&m, ONE_ACTIVE_PER_MACHINE);
            assert_eq!(uniq["type"].as_str(), Some("UniqueConstraint"));
            assert_eq!(
                uniq["fields"],
                serde_json::json!(ONE_ACTIVE_PER_MACHINE_FIELDS)
            );
            assert_eq!(
                uniq["condition"].as_str(),
                Some("(AND: ('revoked_at__isnull', True))")
            );
            assert_eq!(
                ONE_ACTIVE_PER_MACHINE_CONDITION_SQL,
                "\"machine_session\".\"revoked_at\" IS NULL"
            );
        }

        #[test]
        fn indexes_match_fixture() {
            let m = session_fixture();
            let pair = ts::index(&m, MACHINE_REVOKED_INDEX);
            assert_eq!(
                pair["fields"],
                serde_json::json!(MACHINE_REVOKED_INDEX_FIELDS)
            );
            let seen = ts::index(&m, LAST_SEEN_INDEX);
            assert_eq!(seen["fields"], serde_json::json!(LAST_SEEN_INDEX_FIELDS));
        }

        #[test]
        fn defaults_types_and_relations_match_fixture() {
            let m = session_fixture();
            let id = ts::field(&m, "id");
            assert_eq!(id["type"].as_str(), Some("UUIDField"));
            assert_eq!(id["primary_key"].as_bool(), Some(true));
            assert_eq!(id["unique"].as_bool(), Some(true));
            assert_eq!(id["default"].as_str(), Some("<callable uuid4>"));
            let machine = ts::field(&m, "dev_machine");
            assert_eq!(machine["type"].as_str(), Some("ForeignKey"));
            assert_eq!(machine["column"].as_str(), Some(FK_COLUMN));
            assert_eq!(machine["null"].as_bool(), Some(false));
            assert_eq!(machine["related_model"].as_str(), Some("runner.DevMachine"));
            assert_eq!(machine["related_name"].as_str(), Some(FK_RELATED_NAME));
            assert_eq!(machine["on_delete"].as_str(), Some("CASCADE"));
            assert_eq!(FK_ON_DELETE, OnDelete::Cascade);
            assert_eq!(FK_TARGET_TABLE, "dev_machine");
            let proto = ts::field(&m, "protocol_version");
            assert_eq!(proto["type"].as_str(), Some("PositiveIntegerField"));
            assert_eq!(proto["null"].as_bool(), Some(false));
            assert_eq!(proto["default"].as_str(), Some("4"));
            assert_eq!(DEFAULT_PROTOCOL_VERSION, 4);
            let created = ts::field(&m, "created_at");
            assert_eq!(created["type"].as_str(), Some("DateTimeField"));
            assert_eq!(created["null"].as_bool(), Some(false));
            for name in ["last_seen_at", "revoked_at"] {
                let f = ts::field(&m, name);
                assert_eq!(f["type"].as_str(), Some("DateTimeField"), "{name} type");
                assert_eq!(f["null"].as_bool(), Some(true), "{name} null");
            }
            let reason = ts::field(&m, "revoked_reason");
            assert_eq!(reason["type"].as_str(), Some("CharField"));
            assert_eq!(reason["null"].as_bool(), Some(false));
            assert_eq!(reason["max_length"].as_u64(), Some(32));
            assert_eq!(reason["default"].as_str(), Some("''"));
            assert_eq!(REVOKED_REASON_MAX_LENGTH, 32);
            assert_eq!(DEFAULT_REVOKED_REASON, "");
        }

        #[test]
        fn active_id_sql_replays_fixture() {
            let section = &ts::fx05()["active_session_id_for_machine"];
            for side in ["none", "some"] {
                let django = section[side]["sql"][0].as_str().expect("sql");
                assert_eq!(ts::dollarize_param(django), ACTIVE_ID_SQL, "{side} SQL");
            }
            assert!(section["none"]["value"].is_null());
            section["some"]["value"]
                .as_str()
                .expect("uuid")
                .parse::<uuid::Uuid>()
                .expect("some value parses as a UUID");
            assert_eq!(section["some"]["matches"].as_bool(), Some(true));
        }

        #[test]
        fn open_prior_sql_matches_captured_django() {
            let sql = OPEN_PRIOR_SQL;
            assert_full_projection(sql);
            // Byte pin of the WHERE term order (machine term first, captured).
            assert_terms_in_order(
                sql,
                &[
                    "FROM \"machine_session\" WHERE (\"machine_session\".\"dev_machine_id\" = $1 AND \"machine_session\".\"revoked_at\" IS NULL)",
                    "ORDER BY \"machine_session\".\"created_at\" DESC",
                    "LIMIT 1 FOR UPDATE",
                ],
            );
            assert!(sql.ends_with("LIMIT 1 FOR UPDATE"));
        }

        #[test]
        fn open_insert_sql_matches_captured_django() {
            let sql = OPEN_INSERT_SQL;
            assert_terms_in_order(
                sql,
                &[
                    "INSERT INTO \"machine_session\" (\"id\", \"dev_machine_id\", \"protocol_version\", \"created_at\", \"last_seen_at\", \"revoked_at\", \"revoked_reason\")",
                    "VALUES ($1, $2, $3, $4, $5, $6, $7)",
                ],
            );
            assert!(
                !sql.contains("RETURNING"),
                "no RETURNING: the PK is caller-supplied"
            );
        }

        #[test]
        fn revoke_sql_matches_captured_django() {
            // SET order is update_fields order; the WHERE is the bare pk.
            assert_eq!(
                REVOKE_SQL,
                "UPDATE \"machine_session\" SET \"revoked_at\" = $1, \"revoked_reason\" = $2 WHERE \"machine_session\".\"id\" = $3"
            );
        }

        #[test]
        fn delete_get_sql_matches_captured_django() {
            let sql = DELETE_GET_SQL;
            assert_full_projection(sql);
            assert_terms_in_order(
                sql,
                &[
                    "FROM \"machine_session\" WHERE (\"machine_session\".\"dev_machine_id\" = $1 AND \"machine_session\".\"id\" = $2 AND \"machine_session\".\"revoked_at\" IS NULL)",
                    "LIMIT 21",
                ],
            );
            assert!(!sql.contains("ORDER BY"), ".get() clears Meta.ordering");
            assert!(sql.ends_with("LIMIT 21"));
        }

        #[test]
        fn poll_get_sql_matches_captured_django() {
            let sql = POLL_GET_SQL;
            assert_full_projection(sql);
            // No revoked_at term: revoked rows are fetched, then rejected
            // with their reason (views/machine_sessions.py:187-194).
            assert_terms_in_order(
                sql,
                &[
                    "FROM \"machine_session\" WHERE (\"machine_session\".\"dev_machine_id\" = $1 AND \"machine_session\".\"id\" = $2)",
                    "LIMIT 21",
                ],
            );
            assert!(!sql.contains("ORDER BY"), ".get() clears Meta.ordering");
            assert!(sql.ends_with("LIMIT 21"));
        }

        #[test]
        fn touch_sql_matches_captured_django() {
            assert_eq!(
                TOUCH_SQL,
                "UPDATE \"machine_session\" SET \"last_seen_at\" = $1 WHERE \"machine_session\".\"id\" = $2"
            );
        }
    }
}
