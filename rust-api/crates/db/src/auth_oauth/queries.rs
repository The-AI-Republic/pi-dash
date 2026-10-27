//! D-17 authentication query units (db layer, stage 5).
//!
//! Ports the units PIDASHCONV-327 owns:
//!
//! * `OauthAdapter.create_update_account`
//!   (`apps/api/pi_dash/authentication/adapter/oauth.py:105-136`; Account
//!   columns `db/models/user.py:280-302`) — AUTHOAUTH-F6.
//! * `deactivate_api_token`
//!   (`apps/api/pi_dash/authentication/services/cli_tokens.py:1-19`, full
//!   file; call site `views/cli/device.py:476-477`) — AUTHOAUTH-F7
//!   (moved to `pidash_services::domain::tokens::cli_token`,
//!   PIDASHCONV-1520).
//! * `_touch_dev_machine` (`views/cli/device.py:73-85`),
//!   `_get_or_create_dev_machine` (`:88-109`) and the
//!   `_rotate_machine_token` label derivations (`:112-129`; the
//!   statements moved to `pidash_services::domain::tokens`,
//!   PIDASHCONV-1520) — AUTHOAUTH-F8.
//!
//! `Account`, `APIToken`, `DevMachine`, `MachineToken` and `WorkspaceMember`
//! are external models used as-is: this module records only the table names
//! and the columns these statements touch (mirroring the Django field order
//! in the fixtures). It defines no row structs for them.
//!
//! Fixture source of truth: `rust-api/fixtures/auth_oauth/`
//! `F6_account_upsert.before_after.json` (AUTHOAUTH-F6),
//! `F7_deactivate_token.before_after.json` (AUTHOAUTH-F7) and
//! `F8_device_helpers.golden.json` (AUTHOAUTH-F8), recorded by
//! PIDASHCONV-324. The `#[cfg(test)]` suite asserts the SQL text and the
//! pure row/field semantics against those fixtures field-for-field.
//!
//! SQL execution uses runtime `sqlx::query` (no `query!` macros): there is
//! no build-time database and no `.sqlx` offline cache, following the merged
//! `license/queries` precedent. Django's `%s` placeholders render as
//! Postgres `$N` here; the predicate/column order matches the recorded
//! Django SQL exactly.
//!
//! Write routing: callers pick the pool and pass an executor in — every
//! executor is generic over `sqlx::Executor`, except
//! [`dev_machine::get_or_create_dev_machine`], which needs a `&mut
//! PgConnection` to issue the `SAVEPOINT` Django's `transaction.atomic()`
//! would create around the racing insert.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * BUG-6: `create_update_account` swallows `DatabaseError`/`IntegrityError`
//!   via `log_exception` (`adapter/oauth.py:105-136`) — the caller sees no
//!   exception. `domain::identity::account::create_update_account` ports this as-is: a
//!   database failure returns `Ok(UpsertOutcome::ErrorSwallowed)` after a
//!   `tracing::warn`, never `Err`.
//! * The account lookup reads
//!   `self.user_data.get("user").get("provider_id")` (strict: a missing
//!   `"user"` key raises `AttributeError`, uncaught), while the create path
//!   reads `self.user_data.get("user", {}).get("provider_id")` (tolerant).
//!   `domain::identity::account::lookup_provider_id` ports the tolerant create-side form;
//!   callers hitting the strict lookup side with a user-less payload get
//!   `None` back instead of an exception — the only observable difference,
//!   and only on input the providers never emit (every `set_user_data`
//!   stores a `user` dict).
//! * Python `host_label[:255]` / `[:128]` / `[:96]` slice by Unicode code
//!   points and never panic; [`dev_machine::truncate_chars`] mirrors that
//!   with `chars().take(n)`. Byte-slicing would panic on a UTF-8 boundary
//!   (Porting guide semantic trap) and must not be used.
//! * `machine.save(update_fields=[...])` writes exactly the listed columns;
//!   [`dev_machine::TouchFields`] enumerates the four reachable lists so the
//!   emitted `UPDATE` matches Django's column-for-column.

use sqlx::postgres::{PgConnection, PgRow};
use sqlx::Row;

// account — OauthAdapter.create_update_account (AUTHOAUTH-F6)
// ---------------------------------------------------------------------------
// NOTE: the `account` helper (`create_update_account` /
// `rotate_account_tokens`) used to live here; it moved to
// `pidash_services::domain::identity::account` (PIDASHCONV-1525),
// whose tests carry the F6 pins.

// dev_machine — _touch / _get_or_create / _rotate (AUTHOAUTH-F8)
// ---------------------------------------------------------------------------

/// Device-machine helpers (`views/cli/device.py:69-129`).
pub mod dev_machine {
    use super::*;

    /// `DevMachine` table (`runner/models.py`, `Meta.db_table`).
    pub const TABLE: &str = "dev_machine";

    /// `MachineToken` table (`runner/models.py`, `Meta.db_table`).
    pub const MACHINE_TOKEN_TABLE: &str = "machine_token";

    /// `host_label` column width (`CharField(max_length=255)`).
    pub const HOST_LABEL_MAX: usize = 255;
    /// `DevMachine.label` column width (`CharField(max_length=128)`).
    pub const LABEL_MAX: usize = 128;
    /// `MachineToken.label` host slice in `f"machine: {host_label[:96]}"`.
    pub const ROTATE_LABEL_HOST_MAX: usize = 96;

    /// `Visibility.PRIVATE` (`runner/models.py:186-187`): the Django default
    /// stamped on rows the helper creates.
    pub const VISIBILITY_PRIVATE: i16 = 0;
    /// `RunnerProvisioning.MANUAL` (`runner/models.py:190-205`): the Django
    /// default stamped on rows the helper creates.
    pub const PROVISIONING_MANUAL: &str = "manual";

    /// `runner_tokens.mint_machine_token` raw form
    /// (`runner/services/tokens.py:37,84-86`): `MACHINE_TOKEN_PREFIX +
    /// secrets.token_urlsafe(32)`. Minting itself stays runner-owned; the
    /// rotate helper only stores `minted.hashed` / `minted.fingerprint`.
    pub const MINTED_TOKEN_PREFIX: &str = "mt_";

    /// Truncate like Python `s[:n]`: by Unicode code points, never panicking
    /// on a UTF-8 boundary.
    pub fn truncate_chars(s: &str, n: usize) -> String {
        s.chars().take(n).collect()
    }

    /// `(host_label or "").strip()[:255]` (`device.py:74,89`).
    pub fn normalize_host_label(host_label: Option<&str>) -> String {
        truncate_chars(host_label.unwrap_or("").trim(), HOST_LABEL_MAX)
    }

    /// `host_label[:128]` for `DevMachine.label` (`device.py:82,100`).
    pub fn label_from_host_label(normalized: &str) -> String {
        truncate_chars(normalized, LABEL_MAX)
    }

    /// `host_label[:255]` stored on the minted `MachineToken`
    /// (`device.py:121`).
    pub fn rotate_host_label(host_label: &str) -> String {
        truncate_chars(host_label, HOST_LABEL_MAX)
    }

    /// `f"machine: {host_label[:96]}"` (`device.py:125`).
    pub fn machine_token_label(host_label: &str) -> String {
        format!(
            "machine: {}",
            truncate_chars(host_label, ROTATE_LABEL_HOST_MAX)
        )
    }

    /// The four reachable `update_fields` lists for
    /// `machine.save(update_fields=...)` (`device.py:75-84`): always
    /// `last_seen_at` + `updated_at`; `host_label` when the normalized label
    /// is non-empty and differs; `label` when it is non-empty and the row
    /// has none (`not machine.label`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TouchFields {
        Base,
        Host,
        Label,
        HostAndLabel,
    }

    impl TouchFields {
        pub fn as_list(self) -> Vec<&'static str> {
            match self {
                TouchFields::Base => vec!["last_seen_at", "updated_at"],
                TouchFields::Host => vec!["last_seen_at", "updated_at", "host_label"],
                TouchFields::Label => vec!["last_seen_at", "updated_at", "label"],
                TouchFields::HostAndLabel => {
                    vec!["last_seen_at", "updated_at", "host_label", "label"]
                }
            }
        }
    }

    /// Select the `update_fields` list for `_touch_dev_machine`.
    pub fn touch_update_fields(
        current_host_label: &str,
        current_label: &str,
        new_host_label: &str,
    ) -> TouchFields {
        if new_host_label.is_empty() {
            return TouchFields::Base;
        }
        let host_changed = current_host_label != new_host_label;
        let label_missing = current_label.is_empty();
        match (host_changed, label_missing) {
            (true, true) => TouchFields::HostAndLabel,
            (true, false) => TouchFields::Host,
            (false, true) => TouchFields::Label,
            (false, false) => TouchFields::Base,
        }
    }

    /// `DevMachine.objects.select_for_update().filter(pk=$1).first()`
    /// (`device.py:91,105`).
    pub const LOCK_SQL: &str = "SELECT \"dev_machine\".\"id\", \"dev_machine\".\"owner_id\", \"dev_machine\".\"host_label\", \"dev_machine\".\"label\", \"dev_machine\".\"last_seen_at\" FROM \"dev_machine\" WHERE \"dev_machine\".\"id\" = $1 FOR UPDATE LIMIT 1";

    /// `DevMachine.objects.create(id=, owner=, host_label=, label=,
    /// last_seen_at=)` inside `transaction.atomic()` (`device.py:94-101`):
    /// Django fills the remaining columns from field defaults
    /// (`visibility=PRIVATE`, `provisioning=MANUAL`, `created_at`/`updated_at`
    /// = now, `last_seen_at`/`revoked_at` = NULL unless set).
    pub const CREATE_SQL: &str = "INSERT INTO \"dev_machine\" (\"id\", \"owner_id\", \"host_label\", \"label\", \"visibility\", \"provisioning\", \"last_seen_at\", \"revoked_at\", \"created_at\", \"updated_at\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)";

    /// `machine.save(update_fields=[...])` for each reachable list.
    pub const TOUCH_BASE_SQL: &str = "UPDATE \"dev_machine\" SET \"last_seen_at\" = $1, \"updated_at\" = $2 WHERE \"dev_machine\".\"id\" = $3";
    pub const TOUCH_HOST_SQL: &str = "UPDATE \"dev_machine\" SET \"last_seen_at\" = $1, \"updated_at\" = $2, \"host_label\" = $3 WHERE \"dev_machine\".\"id\" = $4";
    pub const TOUCH_LABEL_SQL: &str = "UPDATE \"dev_machine\" SET \"last_seen_at\" = $1, \"updated_at\" = $2, \"label\" = $3 WHERE \"dev_machine\".\"id\" = $4";
    pub const TOUCH_HOST_AND_LABEL_SQL: &str = "UPDATE \"dev_machine\" SET \"last_seen_at\" = $1, \"updated_at\" = $2, \"host_label\" = $3, \"label\" = $4 WHERE \"dev_machine\".\"id\" = $5";

    // NOTE: the rotate statements (`ROTATE_REVOKE_SQL`,
    // `ROTATE_CREATE_SQL`) and the `rotate_machine_token` driver used to
    // live here; they moved to `pidash_services::domain::tokens`
    // (`revoke_tokens_for_device_pair`, `insert_machine_token`;
    // PIDASHCONV-1520), whose tests carry the pins. The label
    // derivations below stay: pure helpers with live callers.

    /// Raised on owner mismatch, on the existing row and on the post-race
    /// row alike (`device.py:69-70`); the caller maps it to 404
    /// `dev_machine_not_found` (`device.py:478-482`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DevMachineOwnershipError;

    impl std::fmt::Display for DevMachineOwnershipError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("dev machine owned by another user")
        }
    }

    impl std::error::Error for DevMachineOwnershipError {}

    /// Owner check shared by the fast path and the post-race path
    /// (`locked.owner_id != user.id → raise`, `device.py:92-93,106-107`).
    pub fn check_owner(
        locked_owner_id: uuid::Uuid,
        user_id: uuid::Uuid,
    ) -> Result<(), DevMachineOwnershipError> {
        if locked_owner_id != user_id {
            return Err(DevMachineOwnershipError);
        }
        Ok(())
    }

    /// Post-`IntegrityError` resolution (`device.py:103-109`): re-`SELECT
    /// ... FOR UPDATE`; `None` or an owner mismatch raises, a same-owner row
    /// is touched.
    pub fn resolve_race(
        relocked_owner_id: Option<uuid::Uuid>,
        user_id: uuid::Uuid,
    ) -> Result<(), DevMachineOwnershipError> {
        match relocked_owner_id {
            Some(owner_id) => check_owner(owner_id, user_id),
            None => Err(DevMachineOwnershipError),
        }
    }

    /// `true` for a Postgres unique-violation (`23505`) — the concurrent
    /// `create` that sends the loser down the re-lock path.
    pub fn is_unique_violation(err: &sqlx::Error) -> bool {
        matches!(err, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
    }

    /// The locked-row working set the touch path needs.
    pub struct LockedMachine {
        pub id: uuid::Uuid,
        pub owner_id: uuid::Uuid,
        pub host_label: String,
        pub label: String,
    }

    /// `SELECT ... FOR UPDATE` by pk, projected to the touch working set.
    pub async fn fetch_locked_machine<'e, E>(
        ex: E,
        dev_machine_id: uuid::Uuid,
    ) -> Result<Option<LockedMachine>, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row: Option<PgRow> = sqlx::query(LOCK_SQL)
            .bind(dev_machine_id)
            .fetch_optional(ex)
            .await?;
        row.map(|r| {
            Ok(LockedMachine {
                id: r.try_get("id")?,
                owner_id: r.try_get("owner_id")?,
                host_label: r.try_get("host_label")?,
                label: r.try_get("label")?,
            })
        })
        .transpose()
    }

    /// `_touch_dev_machine` (`device.py:73-85`): always stamp
    /// `last_seen_at`; conditionally `host_label`/`label`; save exactly the
    /// selected fields.
    pub async fn touch_dev_machine<'e, E>(
        ex: E,
        machine_id: uuid::Uuid,
        current_host_label: &str,
        current_label: &str,
        host_label: Option<&str>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<TouchFields, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let normalized = normalize_host_label(host_label);
        let fields = touch_update_fields(current_host_label, current_label, &normalized);
        match fields {
            TouchFields::Base => {
                sqlx::query(TOUCH_BASE_SQL)
                    .bind(now)
                    .bind(now)
                    .bind(machine_id)
                    .execute(ex)
                    .await?;
            }
            TouchFields::Host => {
                sqlx::query(TOUCH_HOST_SQL)
                    .bind(now)
                    .bind(now)
                    .bind(&normalized)
                    .bind(machine_id)
                    .execute(ex)
                    .await?;
            }
            TouchFields::Label => {
                sqlx::query(TOUCH_LABEL_SQL)
                    .bind(now)
                    .bind(now)
                    .bind(label_from_host_label(&normalized))
                    .bind(machine_id)
                    .execute(ex)
                    .await?;
            }
            TouchFields::HostAndLabel => {
                sqlx::query(TOUCH_HOST_AND_LABEL_SQL)
                    .bind(now)
                    .bind(now)
                    .bind(&normalized)
                    .bind(label_from_host_label(&normalized))
                    .execute(ex)
                    .await?;
            }
        }
        Ok(fields)
    }

    /// How `_get_or_create_dev_machine` finished.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum GetOrCreateOutcome {
        Existing,
        Created,
    }

    /// Parameters for the racing insert (`device.py:94-101`). `id` is the
    /// caller-supplied pk; `now` stands in for `timezone.now()`.
    pub struct NewDevMachine<'a> {
        pub id: uuid::Uuid,
        pub owner_id: uuid::Uuid,
        pub host_label: &'a str,
        pub now: chrono::DateTime<chrono::Utc>,
    }

    /// `_get_or_create_dev_machine` (`device.py:88-109`): lock-then-create-
    /// or-retry. Needs `&mut PgConnection` because the racing insert runs
    /// under a savepoint (what `transaction.atomic()` creates when already
    /// inside a transaction): on a unique violation the savepoint rolls
    /// back and the loser re-locks under the same scope.
    pub async fn get_or_create_dev_machine(
        conn: &mut PgConnection,
        user_id: uuid::Uuid,
        params: &NewDevMachine<'_>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(GetOrCreateOutcome, TouchFields), DevMachineError> {
        let normalized = normalize_host_label(Some(params.host_label));
        if let Some(locked) = fetch_locked_machine(&mut *conn, params.id).await? {
            check_owner(locked.owner_id, user_id)?;
            let fields = touch_dev_machine(
                &mut *conn,
                locked.id,
                &locked.host_label,
                &locked.label,
                Some(&normalized),
                now,
            )
            .await?;
            return Ok((GetOrCreateOutcome::Existing, fields));
        }
        sqlx::query("SAVEPOINT pidash_dev_machine")
            .execute(&mut *conn)
            .await?;
        let insert = sqlx::query(CREATE_SQL)
            .bind(params.id)
            .bind(params.owner_id)
            .bind(&normalized)
            .bind(label_from_host_label(&normalized))
            .bind(VISIBILITY_PRIVATE)
            .bind(PROVISIONING_MANUAL)
            .bind(now)
            .bind(Option::<chrono::DateTime<chrono::Utc>>::None)
            .bind(now)
            .bind(now)
            .execute(&mut *conn)
            .await;
        match insert {
            Ok(_) => {
                sqlx::query("RELEASE SAVEPOINT pidash_dev_machine")
                    .execute(&mut *conn)
                    .await?;
                Ok((GetOrCreateOutcome::Created, TouchFields::Base))
            }
            Err(e) if is_unique_violation(&e) => {
                sqlx::query("ROLLBACK TO SAVEPOINT pidash_dev_machine")
                    .execute(&mut *conn)
                    .await?;
                sqlx::query("RELEASE SAVEPOINT pidash_dev_machine")
                    .execute(&mut *conn)
                    .await?;
                let relocked = fetch_locked_machine(&mut *conn, params.id).await?;
                let owner = relocked.as_ref().map(|m| m.owner_id);
                resolve_race(owner, user_id)?;
                let relocked = relocked.expect("owner checked Some");
                let fields = touch_dev_machine(
                    &mut *conn,
                    relocked.id,
                    &relocked.host_label,
                    &relocked.label,
                    Some(&normalized),
                    now,
                )
                .await?;
                Ok((GetOrCreateOutcome::Existing, fields))
            }
            Err(e) => Err(DevMachineError::Db(e)),
        }
    }

    /// Errors from [`get_or_create_dev_machine`]: database failures pass
    /// through; owner mismatches raise `DevMachineOwnershipError` exactly
    /// like Python (no swallowing here — `device.py:88-109` has no
    /// `try/except` around the ownership raise).
    #[derive(Debug)]
    pub enum DevMachineError {
        Db(sqlx::Error),
        Ownership(DevMachineOwnershipError),
    }

    impl std::fmt::Display for DevMachineError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                DevMachineError::Db(e) => write!(f, "dev machine query failed: {e}"),
                DevMachineError::Ownership(e) => write!(f, "{e}"),
            }
        }
    }

    impl std::error::Error for DevMachineError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match self {
                DevMachineError::Db(e) => Some(e),
                DevMachineError::Ownership(e) => Some(e),
            }
        }
    }

    impl From<sqlx::Error> for DevMachineError {
        fn from(e: sqlx::Error) -> Self {
            DevMachineError::Db(e)
        }
    }

    impl From<DevMachineOwnershipError> for DevMachineError {
        fn from(e: DevMachineOwnershipError) -> Self {
            DevMachineError::Ownership(e)
        }
    }

    // NOTE: `NewMachineToken` / `rotate_machine_token` /
    // `RotateOutcome` used to live here; the statements moved to
    // `pidash_services::domain::tokens` (PIDASHCONV-1520). The driver
    // had no production callers (the device handler inlines the pair
    // on its own transaction).
}

#[cfg(test)]
mod tests {
    use super::super::test_support as ts;
    use super::*;

    fn fixture(name: &str) -> serde_json::Value {
        ts::fixture_file(name)
    }

    // -- AUTHOAUTH-F6: account upsert -----------------------------------------
    // (The SQL/code pins moved with the helper to
    // `pidash_services::domain::identity::account`; only the
    // fixture-shape assertions stay here.)

    #[test]
    fn f6_table_and_lookup_shape_match_fixture() {
        let f6 = fixture("F6_account_upsert.before_after.json");
        assert_eq!(f6["account_table"]["db_table"], "accounts");
        assert_eq!(
            f6["account_table"]["unique_together"],
            serde_json::json!(["provider", "provider_account_id"])
        );
        assert_eq!(
            f6["account_table"]["ordering"],
            serde_json::json!(["-created_at"])
        );
        let lookup = f6["emitted_sql"]["lookup"].as_str().expect("lookup sql");
        assert!(lookup.contains("user_id=%s"), "lookup filters user_id");
        assert!(lookup.contains("provider=%s"), "lookup filters provider");
        assert!(
            lookup.contains("provider_account_id=%s"),
            "lookup filters provider_account_id"
        );
        assert!(
            lookup.ends_with("LIMIT 1 (.first())") || lookup.contains("LIMIT 1"),
            "first() limit"
        );
    }

    #[test]
    fn f6_update_row_shape_match_fixture() {
        let f6 = fixture("F6_account_upsert.before_after.json");
        let after = &f6["after_state"]["update"];
        for col in [
            "access_token",
            "refresh_token",
            "access_token_expired_at",
            "refresh_token_expired_at",
            "last_connected_at",
            "id_token",
        ] {
            assert!(
                after["row"].get(col).is_some(),
                "fixture update row has {col}"
            );
        }
        assert!(
            after["untouched"]
                .as_array()
                .expect("untouched array")
                .iter()
                .any(|col| col.as_str().expect("untouched entry") == "user_id"),
            "fixture marks user_id untouched"
        );
    }

    #[test]
    fn f6_insert_row_shape_match_fixture() {
        let f6 = fixture("F6_account_upsert.before_after.json");
        let create = &f6["after_state"]["create"]["row"];
        for key in [
            "access_token",
            "refresh_token",
            "access_token_expired_at",
            "refresh_token_expired_at",
            "last_connected_at",
            "id_token",
            "provider",
            "provider_account_id",
            "user_id",
        ] {
            assert!(create.get(key).is_some(), "fixture create row has {key}");
        }
    }

    #[test]
    fn f6_error_swallowing_is_documented() {
        let f6 = fixture("F6_account_upsert.before_after.json");
        assert!(
            f6["error_swallowing"]
                .as_str()
                .expect("swallow note")
                .contains("log_exception"),
            "BUG-6 recorded in fixture"
        );
    }

    // -- AUTHOAUTH-F7: deactivate_api_token -----------------------------------
    // (The SQL/noop pins moved with the helper to
    // `pidash_services::domain::tokens::cli_token`; only the
    // fixture-shape assertions stay here.)

    #[test]
    fn f7_before_after_cases_shape() {
        let f7 = fixture("F7_deactivate_token.before_after.json");
        // Match case deactivates exactly the token row; only is_active +
        // updated_at change.
        let m = &f7["after_state"]["match_case"];
        assert_eq!(m["out"], 1);
        assert_eq!(m["rows_after"][0]["is_active"], false);
        assert!(
            m["rows_after"][0].get("updated_at").is_some(),
            "updated_at restamped"
        );
        // Description-guard case leaves a non-device token alone.
        let g = &f7["after_state"]["description_guard_case"];
        assert_eq!(g["out"], 0);
        assert_eq!(g["rows_after"][0]["is_active"], true);
        // Empty-token case short-circuits with no query.
        assert_eq!(f7["after_state"]["empty_token_case"]["out"], 0);
    }

    // -- AUTHOAUTH-F8: device helpers ------------------------------------------

    #[test]
    fn f8_tables_match_fixture() {
        let f8 = fixture("F8_device_helpers.golden.json");
        assert_eq!(dev_machine::TABLE, "dev_machine");
        assert_eq!(dev_machine::MACHINE_TOKEN_TABLE, "machine_token");
        for (key, table) in [
            ("dev_machine_columns", dev_machine::TABLE),
            ("machine_token_columns", dev_machine::MACHINE_TOKEN_TABLE),
        ] {
            let section = f8.get(key).unwrap_or_else(|| panic!("F8 has {key}"));
            assert_eq!(section["db_table"], table, "{key} table name");
        }
        assert_eq!(dev_machine::VISIBILITY_PRIVATE, 0);
        assert_eq!(dev_machine::PROVISIONING_MANUAL, "manual");
        assert_eq!(dev_machine::MINTED_TOKEN_PREFIX, "mt_");
    }

    #[test]
    fn f8_touch_truncation_and_fields() {
        let f8 = fixture("F8_device_helpers.golden.json");
        assert_eq!(
            f8["_touch_dev_machine"]["truncation"]["host_label_field"],
            255
        );
        assert_eq!(f8["_touch_dev_machine"]["truncation"]["label_field"], 128);
        assert_eq!(dev_machine::HOST_LABEL_MAX, 255);
        assert_eq!(dev_machine::LABEL_MAX, 128);
        // (host_label or "").strip()[:255]
        assert_eq!(dev_machine::normalize_host_label(None), "");
        assert_eq!(dev_machine::normalize_host_label(Some("  h  ")), "h");
        assert_eq!(dev_machine::normalize_host_label(Some("")), "");
        assert_eq!(dev_machine::normalize_host_label(Some("  ")), "");
        let long = "é".repeat(300);
        assert_eq!(
            dev_machine::normalize_host_label(Some(&long))
                .chars()
                .count(),
            255
        );
        // update_fields selection across the four cases.
        use dev_machine::TouchFields as TF;
        assert_eq!(dev_machine::touch_update_fields("h", "h", ""), TF::Base);
        assert_eq!(dev_machine::touch_update_fields("a", "a", "b"), TF::Host);
        assert_eq!(dev_machine::touch_update_fields("a", "", "a"), TF::Label);
        assert_eq!(
            dev_machine::touch_update_fields("a", "", "b"),
            TF::HostAndLabel
        );
        assert_eq!(dev_machine::touch_update_fields("a", "x", "a"), TF::Base);
        assert_eq!(TF::Base.as_list(), vec!["last_seen_at", "updated_at"]);
        assert!(TF::HostAndLabel.as_list().contains(&"host_label"));
        assert!(TF::HostAndLabel.as_list().contains(&"label"));
        // label is host_label[:128] of the already-255-truncated value.
        assert_eq!(
            dev_machine::label_from_host_label(&"x".repeat(200)).len(),
            128
        );
    }

    #[test]
    fn f8_get_or_create_race_rules() {
        let owner = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let other = uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        // Fast path + post-race path share the owner check.
        assert!(dev_machine::check_owner(owner, owner).is_ok());
        assert!(dev_machine::check_owner(other, owner).is_err());
        assert!(dev_machine::resolve_race(Some(owner), owner).is_ok());
        assert!(dev_machine::resolve_race(Some(other), owner).is_err());
        assert!(dev_machine::resolve_race(None, owner).is_err());
        // Lock SQL mirrors select_for_update().filter(pk).first().
        assert!(dev_machine::LOCK_SQL.contains("FOR UPDATE"), "row lock");
        assert!(
            dev_machine::LOCK_SQL.contains("WHERE \"dev_machine\".\"id\" = $1"),
            "pk filter"
        );
        assert!(dev_machine::LOCK_SQL.ends_with("LIMIT 1"), "first()");
        // Create pins caller pk + truncated labels (device.py:94-101).
        assert!(
            dev_machine::CREATE_SQL.contains("(\"id\", \"owner_id\", \"host_label\", \"label\""),
            "create columns"
        );
        let f8 = fixture("F8_device_helpers.golden.json");
        let create_cols = f8["_get_or_create_dev_machine"]["create_columns"]
            .as_object()
            .expect("create columns");
        assert_eq!(create_cols["id"], serde_json::json!("caller UUID pk"));
        assert_eq!(create_cols["label"], serde_json::json!("host_label[:128]"));
    }

    #[test]
    fn f8_rotate_revoke_and_label() {
        // (The rotate/revoke SQL pins moved with the statements to
        // `pidash_services::domain::tokens`; the label derivations
        // and fixture columns stay here.)
        let f8 = fixture("F8_device_helpers.golden.json");
        // Label derivations.
        assert_eq!(
            dev_machine::machine_token_label("myhost"),
            "machine: myhost"
        );
        assert_eq!(
            dev_machine::machine_token_label(&"h".repeat(200)).len(),
            "machine: ".len() + 96
        );
        assert_eq!(
            dev_machine::rotate_host_label(&"h".repeat(300))
                .chars()
                .count(),
            255
        );
        let create_cols = f8["_rotate_machine_token"]["create_columns"]
            .as_object()
            .expect("rotate columns");
        assert_eq!(
            create_cols["label"],
            serde_json::json!("f'machine: {host_label[:96]}'")
        );
        assert_eq!(create_cols["is_service"], serde_json::json!(true));
        assert_eq!(f8["_rotate_machine_token"]["mint"], "runner_tokens.mint_machine_token() -> raw='mt_'+token_urlsafe(32) (runner/services/tokens.py:84-86)");
    }
}
