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
//! SQL execution goes through SeaORM entities and the query builder
//! (`rust-api/docs/orm-migration.md`): one-table reads by key/filter,
//! `select_only` + `lock_exclusive` + `FromQueryResult` for the locked
//! read (decode by name), `update_many` for the touch writes, and
//! `ActiveModel` inserts for creation. Rows decode by name so a column
//! add/drop can never silently shift the working set (the #2857 /
//! rustpidash duplicate-column class). Django's column, predicate and
//! SET order matches the recorded SQL exactly; statement-shape pins
//! live in the colocated tests below.
//!
//! Write routing: callers pass a SeaORM [`sea_orm::ConnectionTrait`]
//! (the pool connection or [`crate::orm_tx::TxConn`] over an open sqlx
//! transaction). [`dev_machine::get_or_create_dev_machine`] still needs
//! a `&mut PgConnection` because the racing insert runs under a savepoint
//! (a sqlx nested `begin()`, what Django's `transaction.atomic()` creates);
//! it runs SeaORM on that connection through `TxConn`.
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

use sea_orm::sea_query::Expr;
use sea_orm::{FromQueryResult, Set};
use sqlx::postgres::PgConnection;

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

    /// Builder for `DevMachine.objects.select_for_update().filter(pk=$1).first()`
    /// (`device.py:91,105`): by-pk with `FOR UPDATE`. The column set is the
    /// touch working set (`id`, `owner_id`, `host_label`, `label`) — the
    /// former `last_seen_at` projection never left Rust — so only rendered
    /// columns cross the wire (orm-migration §3.1, `FromQueryResult`
    /// decode by name).
    pub fn lock_machine_query(
        id: uuid::Uuid,
    ) -> sea_orm::Select<crate::entity::dev_machine::Entity> {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
        crate::entity::dev_machine::Entity::find()
            .select_only()
            .columns([
                crate::entity::dev_machine::Column::Id,
                crate::entity::dev_machine::Column::OwnerId,
                crate::entity::dev_machine::Column::HostLabel,
                crate::entity::dev_machine::Column::Label,
            ])
            .filter(crate::entity::dev_machine::Column::Id.eq(id))
            .lock_exclusive()
    }

    /// Builder for `machine.save(update_fields=[...])` (`device.py:73-85`)
    /// for one [`TouchFields`] selection: `SET` order is `last_seen_at`,
    /// `updated_at`, then `host_label`/`label` as selected, `WHERE id = $1`.
    /// The `last_seen_at`/`updated_at` pair is always stamped, exactly the
    /// Django `update_fields` list.
    pub fn touch_update(
        fields: TouchFields,
        machine_id: uuid::Uuid,
        normalized_host_label: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> sea_orm::UpdateMany<crate::entity::dev_machine::Entity> {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
        let mut update = crate::entity::dev_machine::Entity::update_many()
            .col_expr(
                crate::entity::dev_machine::Column::LastSeenAt,
                Expr::value(now),
            )
            .col_expr(
                crate::entity::dev_machine::Column::UpdatedAt,
                Expr::value(now),
            );
        match fields {
            TouchFields::Base => {}
            TouchFields::Host => {
                update = update.col_expr(
                    crate::entity::dev_machine::Column::HostLabel,
                    Expr::value(normalized_host_label.to_owned()),
                );
            }
            TouchFields::Label => {
                update = update.col_expr(
                    crate::entity::dev_machine::Column::Label,
                    Expr::value(label_from_host_label(normalized_host_label)),
                );
            }
            TouchFields::HostAndLabel => {
                update = update
                    .col_expr(
                        crate::entity::dev_machine::Column::HostLabel,
                        Expr::value(normalized_host_label.to_owned()),
                    )
                    .col_expr(
                        crate::entity::dev_machine::Column::Label,
                        Expr::value(label_from_host_label(normalized_host_label)),
                    );
            }
        }
        update.filter(crate::entity::dev_machine::Column::Id.eq(machine_id))
    }

    /// The `ActiveModel` for `DevMachine.objects.create(id=, owner=,
    /// host_label=, label=, last_seen_at=)` (`device.py:94-101`): Django
    /// fills every other column from field defaults — `visibility=PRIVATE`
    /// (`0`), `provisioning=MANUAL`, the engine columns (`False`/`""`/
    /// `"never"`/`{}`), `revoked_at=NULL`, `created_at`/`updated_at=now`.
    /// The live table carries no `column_default`, so the insert supplies
    /// them explicitly (the `catalog_reads::insert_dev_machine` precedent).
    pub fn new_machine_model(
        params: &NewDevMachine<'_>,
        normalized_host_label: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> crate::entity::dev_machine::ActiveModel {
        crate::entity::dev_machine::ActiveModel {
            id: Set(params.id),
            owner_id: Set(params.owner_id),
            host_label: Set(normalized_host_label.to_owned()),
            label: Set(label_from_host_label(normalized_host_label)),
            visibility: Set(VISIBILITY_PRIVATE),
            provisioning: Set(PROVISIONING_MANUAL.to_owned()),
            engine_paused_by_owner: Set(false),
            engine_auto_pause_reason: Set(String::new()),
            engine_team_runs: Set(false),
            engine_sign_in_state: Set("never".to_owned()),
            engine_credential_expires_at: Set(None),
            engine_state: Set(serde_json::json!({})),
            engine_state_reported_at: Set(None),
            merged_into_id: Set(None),
            merge_result: Set(serde_json::json!({})),
            last_seen_at: Set(Some(now.into())),
            revoked_at: Set(None),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        }
    }

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

    /// Canonical unique-violation classifier, re-exported from
    /// `pidash_compat::db` (OVL-04) so this public path keeps working.
    pub use pidash_compat::db::is_unique_violation;

    /// The locked-row working set the touch path needs. Decoded by name
    /// from [`lock_machine_query`] via `FromQueryResult` (no positional
    /// reads: the #2857 class cannot go stale when a column moves).
    #[derive(Debug, Clone, PartialEq, Eq, FromQueryResult)]
    pub struct LockedMachine {
        pub id: uuid::Uuid,
        pub owner_id: uuid::Uuid,
        pub host_label: String,
        pub label: String,
    }

    /// `SELECT ... FOR UPDATE` by pk, projected to the touch working set.
    /// `db` is the caller's `DatabaseConnection` or a `TxConn` over its
    /// sqlx transaction — the caller keeps the lock inside its `atomic()`.
    pub async fn fetch_locked_machine(
        db: &impl sea_orm::ConnectionTrait,
        dev_machine_id: uuid::Uuid,
    ) -> Result<Option<LockedMachine>, sea_orm::DbErr> {
        lock_machine_query(dev_machine_id)
            .into_model::<LockedMachine>()
            .one(db)
            .await
    }

    /// `_touch_dev_machine` (`device.py:73-85`): always stamp
    /// `last_seen_at`/`updated_at`; conditionally `host_label`/`label`;
    /// save exactly the selected fields.
    pub async fn touch_dev_machine(
        db: &impl sea_orm::ConnectionTrait,
        machine_id: uuid::Uuid,
        current_host_label: &str,
        current_label: &str,
        host_label: Option<&str>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<TouchFields, sea_orm::DbErr> {
        let normalized = normalize_host_label(host_label);
        let fields = touch_update_fields(current_host_label, current_label, &normalized);
        touch_update(fields, machine_id, &normalized, now)
            .exec(db)
            .await
            .map(|_| fields)
    }

    /// Insert one dev machine (`device.py:97-101` wants `insert` without
    /// `RETURNING` semantics beyond the pk — the row id is caller-supplied,
    /// nothing reads it back). `normalized_host_label` must already be
    /// [`normalize_host_label`] output; `label` derives as `[:128]`.
    pub async fn insert_dev_machine(
        db: &impl sea_orm::ConnectionTrait,
        params: &NewDevMachine<'_>,
        normalized_host_label: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), sea_orm::DbErr> {
        use sea_orm::EntityTrait;
        crate::entity::dev_machine::Entity::insert(new_machine_model(
            params,
            normalized_host_label,
            now,
        ))
        .exec_without_returning(db)
        .await
        .map(drop)
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
    /// under a savepoint (a sqlx nested `begin()`, what
    /// `transaction.atomic()` creates when already inside a transaction):
    /// on a unique violation the savepoint rolls back and the loser
    /// re-locks under the same scope. All statements are SeaORM (`TxConn`
    /// over the connection / savepoint).
    pub async fn get_or_create_dev_machine(
        conn: &mut PgConnection,
        user_id: uuid::Uuid,
        params: &NewDevMachine<'_>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(GetOrCreateOutcome, TouchFields), DevMachineError> {
        use sqlx::Connection as _;
        let normalized = normalize_host_label(Some(params.host_label));
        let locked = {
            let db = crate::orm_tx::TxConn::new(&mut *conn);
            fetch_locked_machine(&db, params.id)
                .await
                .map_err(|e| DevMachineError::Db(crate::orm_tx::into_sqlx(e)))?
        };
        if let Some(locked) = locked {
            check_owner(locked.owner_id, user_id)?;
            let fields = {
                let db = crate::orm_tx::TxConn::new(&mut *conn);
                touch_dev_machine(
                    &db,
                    locked.id,
                    &locked.host_label,
                    &locked.label,
                    Some(&normalized),
                    now,
                )
                .await
                .map_err(|e| DevMachineError::Db(crate::orm_tx::into_sqlx(e)))?
            };
            return Ok((GetOrCreateOutcome::Existing, fields));
        }
        let mut savepoint = conn.begin().await.map_err(DevMachineError::Db)?;
        let created = {
            let db = crate::orm_tx::TxConn::new(&mut savepoint);
            insert_dev_machine(&db, params, &normalized, now).await
        };
        match created {
            Ok(()) => {
                savepoint.commit().await.map_err(DevMachineError::Db)?;
                Ok((GetOrCreateOutcome::Created, TouchFields::Base))
            }
            Err(e)
                if {
                    use sea_orm::{DbErr, RuntimeErr};
                    match &e {
                        DbErr::Exec(RuntimeErr::SqlxError(inner)) => is_unique_violation(inner),
                        DbErr::Query(RuntimeErr::SqlxError(inner)) => is_unique_violation(inner),
                        DbErr::Conn(RuntimeErr::SqlxError(inner)) => is_unique_violation(inner),
                        _ => false,
                    }
                } =>
            {
                savepoint.rollback().await.map_err(DevMachineError::Db)?;
                let relocked = {
                    let db = crate::orm_tx::TxConn::new(&mut *conn);
                    fetch_locked_machine(&db, params.id)
                        .await
                        .map_err(|e| DevMachineError::Db(crate::orm_tx::into_sqlx(e)))?
                };
                let owner = relocked.as_ref().map(|m| m.owner_id);
                resolve_race(owner, user_id)?;
                let relocked = relocked.expect("owner checked Some");
                let fields = {
                    let db = crate::orm_tx::TxConn::new(&mut *conn);
                    touch_dev_machine(
                        &db,
                        relocked.id,
                        &relocked.host_label,
                        &relocked.label,
                        Some(&normalized),
                        now,
                    )
                    .await
                    .map_err(|e| DevMachineError::Db(crate::orm_tx::into_sqlx(e)))?
                };
                Ok((GetOrCreateOutcome::Existing, fields))
            }
            Err(e) => {
                let _ = savepoint.rollback().await;
                Err(DevMachineError::Db(crate::orm_tx::into_sqlx(e)))
            }
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
    use sea_orm::EntityTrait as _;
    use sea_orm::QueryTrait as _;

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
        // Lock statement mirrors select_for_update().filter(pk).first():
        // pk filter + FOR UPDATE; `.one()` adds `LIMIT 1` at execution.
        let lock_sql = dev_machine::lock_machine_query(
            uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
        )
        .build(sea_orm::DbBackend::Postgres)
        .to_string();
        assert!(lock_sql.contains("FOR UPDATE"), "row lock: {lock_sql}");
        assert!(
            lock_sql.contains("\"dev_machine\".\"id\""),
            "pk filter: {lock_sql}"
        );
        assert!(
            lock_sql.contains("\"owner_id\"") && lock_sql.contains("\"host_label\""),
            "working-set columns: {lock_sql}"
        );
        // Create pins caller pk + truncated labels (device.py:94-101).
        let model = dev_machine::new_machine_model(
            &dev_machine::NewDevMachine {
                id: uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
                owner_id: owner,
                host_label: "myhost",
                now: chrono::Utc::now(),
            },
            "myhost",
            chrono::Utc::now(),
        );
        let cols = crate::entity::dev_machine::Entity::insert(model)
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert!(
            cols.contains("\"id\"") && cols.contains("\"owner_id\""),
            "create has caller pk: {cols}"
        );
        assert!(
            cols.contains("\"visibility\"") && cols.contains("\"provisioning\""),
            "create has Django defaults: {cols}"
        );
        // Touch builders emit exactly the Django update_fields subset per case.
        let id = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let now = chrono::Utc::now();
        let base = dev_machine::touch_update(dev_machine::TouchFields::Base, id, "b", now)
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert!(
            base.contains("\"last_seen_at\"") && base.contains("\"updated_at\""),
            "base stamps both timestamps: {base}"
        );
        assert!(
            !base.contains("\"host_label\""),
            "base never writes host_label: {base}"
        );
        let host = dev_machine::touch_update(dev_machine::TouchFields::Host, id, "b", now)
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert!(
            host.contains("\"host_label\"") && !host.contains("\"label\""),
            "host writes host only: {host}"
        );
        let label = dev_machine::touch_update(dev_machine::TouchFields::Label, id, "b", now)
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert!(
            label.contains("\"label\"") && !label.contains("\"host_label\""),
            "label writes label only: {label}"
        );
        let both = dev_machine::touch_update(dev_machine::TouchFields::HostAndLabel, id, "b", now)
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert!(
            both.contains("\"host_label\"") && both.contains("\"label\""),
            "host+label writes both: {both}"
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

    /// Live DB: locked fetch + touch + get-or-create round-trip on a temp
    /// `dev_machine` table (the `orm_tx` precedent — `LIKE ... INCLUDING
    /// DEFAULTS` shadows the real table without FK parents). Skips without
    /// `DATABASE_URL` on a migrated database.
    #[tokio::test]
    async fn live_dev_machine_fetch_touch_get_or_create() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::PgPool::connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        let migrated: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('public.dev_machine')::text")
                .fetch_one(&pool)
                .await
                .expect("probe");
        if migrated.is_none() {
            eprintln!("skipping live-db test: the database is not migrated");
            return;
        }
        let mut tx = pool.begin().await.expect("begin");
        sqlx::query(
            "CREATE TEMP TABLE dev_machine (LIKE public.dev_machine INCLUDING DEFAULTS) ON COMMIT DROP",
        )
        .execute(&mut *tx)
        .await
        .expect("temp dev_machine");
        let owner_id = uuid::Uuid::new_v4();
        let machine_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now();
        // Miss path: lock fetch returns None (no row).
        {
            let db = crate::orm_tx::TxConn::new(&mut tx);
            let missed = dev_machine::fetch_locked_machine(&db, machine_id)
                .await
                .expect("miss fetch");
            assert!(missed.is_none());
        }
        // Create path: get_or_create inserts (Created, Base).
        {
            let (outcome, fields) = dev_machine::get_or_create_dev_machine(
                &mut tx,
                owner_id,
                &dev_machine::NewDevMachine {
                    id: machine_id,
                    owner_id,
                    host_label: "  testhost  ",
                    now,
                },
                now,
            )
            .await
            .expect("create");
            assert_eq!(outcome, dev_machine::GetOrCreateOutcome::Created);
            assert_eq!(fields, dev_machine::TouchFields::Base);
        }
        // Existing path: same owner touches (Existing, HostAndLabel when
        // host changed and label present is false would be label-only;
        // here host "testhost" is already stored so Base after same value).
        {
            let db = crate::orm_tx::TxConn::new(&mut tx);
            let locked = dev_machine::fetch_locked_machine(&db, machine_id)
                .await
                .expect("locked fetch")
                .expect("row exists");
            assert_eq!(locked.owner_id, owner_id);
            assert_eq!(locked.host_label, "testhost");
            assert_eq!(locked.label, "testhost");
            let fields = dev_machine::touch_dev_machine(
                &db,
                locked.id,
                &locked.host_label,
                &locked.label,
                Some("  newhost  "),
                now,
            )
            .await
            .expect("touch host");
            assert_eq!(fields, dev_machine::TouchFields::Host);
            let relocked = dev_machine::fetch_locked_machine(&db, machine_id)
                .await
                .expect("relock")
                .expect("row still exists");
            assert_eq!(relocked.host_label, "newhost");
            assert_eq!(relocked.label, "testhost");
        }
        // Wrong owner is a 404 error, not a silent miss.
        {
            let other = uuid::Uuid::new_v4();
            let outcome = dev_machine::get_or_create_dev_machine(
                &mut tx,
                other,
                &dev_machine::NewDevMachine {
                    id: machine_id,
                    owner_id: other,
                    host_label: "newhost",
                    now,
                },
                now,
            )
            .await;
            assert!(matches!(
                outcome,
                Err(dev_machine::DevMachineError::Ownership(_))
            ));
        }
        tx.rollback().await.expect("rollback");
    }
}
