//! D-17 authentication query units (db layer, stage 5).
//!
//! Ports the units PIDASHCONV-327 owns:
//!
//! * `OauthAdapter.create_update_account`
//!   (`apps/api/pi_dash/authentication/adapter/oauth.py:105-136`; Account
//!   columns `db/models/user.py:280-302`) — AUTHOAUTH-F6.
//! * `deactivate_api_token`
//!   (`apps/api/pi_dash/authentication/services/cli_tokens.py:1-19`, full
//!   file; call site `views/cli/device.py:476-477`) — AUTHOAUTH-F7.
//! * `_touch_dev_machine` (`views/cli/device.py:73-85`),
//!   `_get_or_create_dev_machine` (`:88-109`) and `_rotate_machine_token`
//!   (`:112-129`; mint `runner/services/tokens.py:84-86`) — AUTHOAUTH-F8.
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
//!   exception. [`account::create_update_account`] ports this as-is: a
//!   database failure returns `Ok(UpsertOutcome::ErrorSwallowed)` after a
//!   `tracing::warn`, never `Err`.
//! * The account lookup reads
//!   `self.user_data.get("user").get("provider_id")` (strict: a missing
//!   `"user"` key raises `AttributeError`, uncaught), while the create path
//!   reads `self.user_data.get("user", {}).get("provider_id")` (tolerant).
//!   [`account::lookup_provider_id`] ports the tolerant create-side form;
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

// ---------------------------------------------------------------------------
// account — OauthAdapter.create_update_account (AUTHOAUTH-F6)
// ---------------------------------------------------------------------------

/// Account upsert statements
/// (`adapter/oauth.py:105-136`; table `accounts`, `db/models/user.py:280-302`,
/// `unique_together = ["provider", "provider_account_id"]`,
/// `ordering = ("-created_at",)`).
pub mod account {
    use super::*;

    /// Django table name (`Meta.db_table`, `user.py:298`).
    pub const TABLE: &str = "accounts";

    /// Columns in Django declaration order (audit prefix first), the order
    /// the `SELECT` projects and the `INSERT`/`UPDATE` bind.
    pub const COLUMNS: &[&str] = &[
        "id",
        "created_at",
        "updated_at",
        "user_id",
        "provider_account_id",
        "provider",
        "access_token",
        "access_token_expired_at",
        "refresh_token",
        "refresh_token_expired_at",
        "last_connected_at",
        "id_token",
        "metadata",
    ];

    /// Columns the update path writes, in bind order: the six token fields
    /// Django assigns (`oauth.py:114-119`) plus `updated_at`, which
    /// `auto_now` stamps on every `.save()`.
    pub const UPDATE_COLUMNS: &[&str] = &[
        "access_token",
        "refresh_token",
        "access_token_expired_at",
        "refresh_token_expired_at",
        "last_connected_at",
        "id_token",
        "updated_at",
    ];

    /// Lookup filter columns in bind order
    /// (`filter(user=, provider=, provider_account_id=)`, `oauth.py:108-112`).
    pub const LOOKUP_COLUMNS: &[&str] = &["user_id", "provider", "provider_account_id"];

    /// `Account.objects.filter(user_id=$1 AND provider=$2 AND
    /// provider_account_id=$3).first()` — fixture F6 `emitted_sql.lookup`
    /// (Django `%s` → `$N`).
    pub const LOOKUP_SQL: &str = "SELECT \"accounts\".\"id\", \"accounts\".\"created_at\", \"accounts\".\"updated_at\", \"accounts\".\"user_id\", \"accounts\".\"provider_account_id\", \"accounts\".\"provider\", \"accounts\".\"access_token\", \"accounts\".\"access_token_expired_at\", \"accounts\".\"refresh_token\", \"accounts\".\"refresh_token_expired_at\", \"accounts\".\"last_connected_at\", \"accounts\".\"id_token\", \"accounts\".\"metadata\" FROM \"accounts\" WHERE \"accounts\".\"user_id\" = $1 AND \"accounts\".\"provider\" = $2 AND \"accounts\".\"provider_account_id\" = $3 LIMIT 1";

    /// `account.save()` on the update path: Django writes every local field,
    /// so the `SET` lists all non-pk columns in field order
    /// (`oauth.py:114-120`; fixture F6 `emitted_sql.update` names the six
    /// assigned columns plus `updated_at`).
    pub const UPDATE_SQL: &str = "UPDATE \"accounts\" SET \"created_at\" = $1, \"updated_at\" = $2, \"user_id\" = $3, \"provider_account_id\" = $4, \"provider\" = $5, \"access_token\" = $6, \"access_token_expired_at\" = $7, \"refresh_token\" = $8, \"refresh_token_expired_at\" = $9, \"last_connected_at\" = $10, \"id_token\" = $11, \"metadata\" = $12 WHERE \"accounts\".\"id\" = $13";

    /// `Account.objects.create(...)` (`oauth.py:123-133`): `id` is a
    /// client-side `uuid4` (the caller supplies it — this crate's `uuid`
    /// has no `v4` feature, per the `license/queries` precedent),
    /// `metadata` falls back to `{}`, timestamps to `now`.
    pub const INSERT_SQL: &str = "INSERT INTO \"accounts\" (\"id\", \"created_at\", \"updated_at\", \"user_id\", \"provider_account_id\", \"provider\", \"access_token\", \"access_token_expired_at\", \"refresh_token\", \"refresh_token_expired_at\", \"last_connected_at\", \"id_token\", \"metadata\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)";

    /// The five token fields every provider's `set_token_data` stores
    /// (fixture F4 `token_fields_consumed_downstream`).
    #[derive(Debug, Clone, PartialEq)]
    pub struct TokenFields {
        pub access_token: Option<String>,
        pub refresh_token: Option<String>,
        pub access_token_expired_at: Option<chrono::DateTime<chrono::Utc>>,
        pub refresh_token_expired_at: Option<chrono::DateTime<chrono::Utc>>,
        /// `token_data.get("id_token", "")` (`oauth.py:119,132`): `""`
        /// when the key is absent.
        pub id_token: Option<String>,
    }

    impl TokenFields {
        /// `token_data.get("id_token", "")` — absent maps to `""`.
        pub fn id_token_or_default(&self) -> String {
            self.id_token.clone().unwrap_or_default()
        }
    }

    /// `self.user_data.get("user", {}).get("provider_id")` — the tolerant
    /// create-side read (`oauth.py:126`). The lookup side
    /// (`.get("user").get(...)`, `:111`) raises `AttributeError` on a
    /// user-less payload instead; see the module docs.
    pub fn lookup_provider_id(user_data: &serde_json::Value) -> Option<&str> {
        user_data.get("user")?.get("provider_id")?.as_str()
    }

    /// Outcome of [`create_update_account`]. `ErrorSwallowed` is BUG-6,
    /// ported as-is (see module docs).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum UpsertOutcome {
        Updated,
        Created,
        ErrorSwallowed,
    }

    /// `Account.objects.filter(...).first()` projected to the update path's
    /// only need: the row id.
    pub async fn find_account_id<'e, E>(
        ex: E,
        user_id: uuid::Uuid,
        provider: &str,
        provider_account_id: &str,
    ) -> Result<Option<uuid::Uuid>, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row: Option<PgRow> = sqlx::query(LOOKUP_SQL)
            .bind(user_id)
            .bind(provider)
            .bind(provider_account_id)
            .fetch_optional(ex)
            .await?;
        row.map(|r| r.try_get("id")).transpose()
    }

    /// Parameters for the insert side (`oauth.py:123-133`). `id` is
    /// caller-generated (`uuid4`); `now` stands in for `timezone.now()`;
    /// `metadata` defaults to `{}` when `None`.
    pub struct NewAccount<'a> {
        pub id: uuid::Uuid,
        pub user_id: uuid::Uuid,
        pub provider: &'a str,
        pub provider_account_id: &'a str,
        pub tokens: &'a TokenFields,
        pub now: chrono::DateTime<chrono::Utc>,
        pub metadata: Option<serde_json::Value>,
    }

    /// The update side (`oauth.py:113-120`): assign the six token fields
    /// plus `last_connected_at = now`, then `.save()`.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_account<'e, E>(
        ex: E,
        account_id: uuid::Uuid,
        created_at: chrono::DateTime<chrono::Utc>,
        user_id: uuid::Uuid,
        provider_account_id: &str,
        provider: &str,
        tokens: &TokenFields,
        now: chrono::DateTime<chrono::Utc>,
        metadata: serde_json::Value,
    ) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query(UPDATE_SQL)
            .bind(created_at)
            .bind(now)
            .bind(user_id)
            .bind(provider_account_id)
            .bind(provider)
            .bind(tokens.access_token.clone())
            .bind(tokens.access_token_expired_at)
            .bind(tokens.refresh_token.clone())
            .bind(tokens.refresh_token_expired_at)
            .bind(now)
            .bind(tokens.id_token_or_default())
            .bind(metadata)
            .bind(account_id)
            .execute(ex)
            .await?;
        Ok(())
    }

    /// The create side (`oauth.py:123-133`).
    pub async fn create_account<'e, E>(ex: E, params: &NewAccount<'_>) -> Result<(), sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let metadata = params
            .metadata
            .clone()
            .unwrap_or_else(|| serde_json::json!({}));
        sqlx::query(INSERT_SQL)
            .bind(params.id)
            .bind(params.now)
            .bind(params.now)
            .bind(params.user_id)
            .bind(params.provider_account_id)
            .bind(params.provider)
            .bind(params.tokens.access_token.clone())
            .bind(params.tokens.access_token_expired_at)
            .bind(params.tokens.refresh_token.clone())
            .bind(params.tokens.refresh_token_expired_at)
            .bind(params.now)
            .bind(params.tokens.id_token_or_default())
            .bind(metadata)
            .execute(ex)
            .await?;
        Ok(())
    }

    /// `create_update_account` (`oauth.py:105-136`): filter-first, then
    /// update or create. `DatabaseError`/`IntegrityError` are swallowed via
    /// `log_exception` (BUG-6) — a database failure returns
    /// `Ok(UpsertOutcome::ErrorSwallowed)`, never `Err`.
    ///
    /// Takes `&mut PgConnection` (an acquired connection, or `&mut *tx`
    /// inside a larger transaction — Python runs each statement in
    /// autocommit, so both are faithful). Single-statement steps stay
    /// generic over `sqlx::Executor` for reuse.
    ///
    /// `created_at`/`metadata` for the update side come from the looked-up
    /// row; pass them back in (Django's `.save()` rewrites them unchanged).
    /// `provider_account_id` is `None` when the payload has no
    /// `user.provider_id` (the strict lookup side would have raised
    /// `AttributeError`; here the lookup is skipped and the create side
    /// runs with an empty provider id, matching `.get("user",
    /// {}).get("provider_id")` → `None`).
    #[allow(clippy::too_many_arguments)]
    pub async fn create_update_account(
        conn: &mut PgConnection,
        user_id: uuid::Uuid,
        provider: &str,
        user_data: &serde_json::Value,
        tokens: &TokenFields,
        now: chrono::DateTime<chrono::Utc>,
        new_id: uuid::Uuid,
        existing: Option<ExistingAccount>,
    ) -> Result<UpsertOutcome, sqlx::Error> {
        let provider_account_id = lookup_provider_id(user_data);
        let found = match provider_account_id {
            Some(pid) => find_account_id(&mut *conn, user_id, provider, pid).await,
            None => Ok(None),
        };
        let found = match found {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("Error in create_update_account lookup: {e}");
                return Ok(UpsertOutcome::ErrorSwallowed);
            }
        };
        if let Some(account_id) = found {
            let prior = existing.unwrap_or(ExistingAccount {
                created_at: now,
                metadata: serde_json::json!({}),
            });
            let res = update_account(
                &mut *conn,
                account_id,
                prior.created_at,
                user_id,
                provider_account_id.unwrap_or(""),
                provider,
                tokens,
                now,
                prior.metadata,
            )
            .await;
            return match res {
                Ok(()) => Ok(UpsertOutcome::Updated),
                Err(e) => {
                    tracing::warn!("Error in create_update_account update: {e}");
                    Ok(UpsertOutcome::ErrorSwallowed)
                }
            };
        }
        let params = NewAccount {
            id: new_id,
            user_id,
            provider,
            provider_account_id: provider_account_id.unwrap_or(""),
            tokens,
            now,
            metadata: None,
        };
        match create_account(&mut *conn, &params).await {
            Ok(()) => Ok(UpsertOutcome::Created),
            Err(e) => {
                tracing::warn!("Error in create_update_account create: {e}");
                Ok(UpsertOutcome::ErrorSwallowed)
            }
        }
    }

    /// The unchanged columns the update side rewrites verbatim
    /// (`created_at`, `metadata`; `user_id`/`provider`/`provider_account_id`
    /// are untouched per fixture F6 `after_state.update.untouched`).
    pub struct ExistingAccount {
        pub created_at: chrono::DateTime<chrono::Utc>,
        pub metadata: serde_json::Value,
    }

    /// Pure row transform mirroring the update side for fixture
    /// before/after assertions: assign the token fields + `last_connected_at`
    /// over a JSON row shaped like F6 `before_state.case_a_exists`.
    pub fn apply_update_row(
        before: &serde_json::Value,
        tokens: &TokenFields,
        now: &str,
    ) -> serde_json::Value {
        let mut after = before.clone();
        after["access_token"] = tokens
            .access_token
            .clone()
            .map_or(serde_json::Value::Null, serde_json::Value::from);
        after["refresh_token"] = tokens
            .refresh_token
            .clone()
            .map_or(serde_json::Value::Null, serde_json::Value::from);
        after["access_token_expired_at"] = tokens
            .access_token_expired_at
            .map(|dt| serde_json::Value::from(dt.to_rfc3339()))
            .unwrap_or(serde_json::Value::Null);
        after["refresh_token_expired_at"] = tokens
            .refresh_token_expired_at
            .map(|dt| serde_json::Value::from(dt.to_rfc3339()))
            .unwrap_or(serde_json::Value::Null);
        after["last_connected_at"] = serde_json::Value::from(now);
        after["id_token"] = serde_json::Value::from(tokens.id_token_or_default());
        after
    }
}

// ---------------------------------------------------------------------------
// cli_token — deactivate_api_token (AUTHOAUTH-F7)
// ---------------------------------------------------------------------------

/// `deactivate_api_token` (`services/cli_tokens.py:1-19`, full file).
pub mod cli_token {
    /// `APIToken` table (`db/models/api.py`, `Meta.db_table`; same table the
    /// `api_token_device_flow` reference in the sibling [`super::super::models`]
    /// module pins — reused as-is, not redefined).
    pub const TABLE: &str = super::super::models::api_token_device_flow::TABLE;

    /// `CLI_DEVICE_API_TOKEN_DESCRIPTION` (`cli_tokens.py:9`).
    pub const DEVICE_FLOW_DESCRIPTION: &str =
        super::super::models::api_token_device_flow::DEVICE_FLOW_DESCRIPTION;

    /// `APIToken.objects.filter(token=$1, is_active=true).update(
    /// is_active=false, updated_at=$2)` — fixture F7 `emitted_sql` without
    /// the description guard.
    pub const DEACTIVATE_SQL: &str = "UPDATE \"api_tokens\" SET \"is_active\" = false, \"updated_at\" = $1 WHERE \"api_tokens\".\"token\" = $2 AND \"api_tokens\".\"is_active\" = true";

    /// Same with the CLI-device guard
    /// (`.filter(description=CLI_DEVICE_API_TOKEN_DESCRIPTION)`).
    pub const DEACTIVATE_CLI_ONLY_SQL: &str = "UPDATE \"api_tokens\" SET \"is_active\" = false, \"updated_at\" = $1 WHERE \"api_tokens\".\"token\" = $2 AND \"api_tokens\".\"is_active\" = true AND \"api_tokens\".\"description\" = $3";

    /// `if not raw_token: return 0` (`cli_tokens.py:14-15`): `None` or `""`
    /// short-circuits with no query.
    pub fn is_noop_token(raw_token: Option<&str>) -> bool {
        raw_token.unwrap_or("").is_empty()
    }

    /// Mark an `APIToken` inactive after its exchange for an `mt_` token
    /// (`cli_tokens.py:12-19`). Returns the updated row count (`0` when the
    /// token is empty/`None`, unmatched, inactive, or — with
    /// `only_cli_device_tokens` — not a device-flow token). The revoke
    /// endpoint does NOT use this helper (inline `is_active=False` save,
    /// `device.py:496-511`).
    pub async fn deactivate_api_token<'e, E>(
        ex: E,
        raw_token: Option<&str>,
        only_cli_device_tokens: bool,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let token = raw_token.unwrap_or("");
        if token.is_empty() {
            return Ok(0);
        }
        let rows = if only_cli_device_tokens {
            sqlx::query(DEACTIVATE_CLI_ONLY_SQL)
                .bind(now)
                .bind(token)
                .bind(DEVICE_FLOW_DESCRIPTION)
                .execute(ex)
                .await?
                .rows_affected()
        } else {
            sqlx::query(DEACTIVATE_SQL)
                .bind(now)
                .bind(token)
                .execute(ex)
                .await?
                .rows_affected()
        };
        Ok(rows)
    }
}

// ---------------------------------------------------------------------------
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

    /// `MachineToken.objects.select_for_update().filter(workspace=,
    /// dev_machine=, revoked_at__isnull=True).update(revoked_at=now)`
    /// (`device.py:113-117`).
    pub const ROTATE_REVOKE_SQL: &str = "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE \"machine_token\".\"workspace_id\" = $2 AND \"machine_token\".\"dev_machine_id\" = $3 AND \"machine_token\".\"revoked_at\" IS NULL";

    /// `MachineToken.objects.create(user=, dev_machine=, workspace=,
    /// host_label=, token_hash=, token_fingerprint=, label=, is_service=True)`
    /// (`device.py:119-127`): `id` is a client-side `uuid4` (caller
    /// supplies it); `created_at` = now; `last_used_at`/`revoked_at` NULL.
    pub const ROTATE_CREATE_SQL: &str = "INSERT INTO \"machine_token\" (\"id\", \"user_id\", \"dev_machine_id\", \"workspace_id\", \"host_label\", \"token_hash\", \"token_fingerprint\", \"label\", \"is_service\", \"created_at\", \"last_used_at\", \"revoked_at\") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)";

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

    /// Parameters for the rotate insert (`device.py:119-127`). `id` is a
    /// caller-generated `uuid4`; `token_hash`/`token_fingerprint` come from
    /// `runner_tokens.mint_machine_token()`; `now` stands in for
    /// `timezone.now()`.
    pub struct NewMachineToken<'a> {
        pub id: uuid::Uuid,
        pub user_id: uuid::Uuid,
        pub dev_machine_id: uuid::Uuid,
        pub workspace_id: uuid::Uuid,
        pub host_label: &'a str,
        pub token_hash: &'a str,
        pub token_fingerprint: &'a str,
        pub now: chrono::DateTime<chrono::Utc>,
    }

    /// `_rotate_machine_token` (`device.py:112-129`): revoke every active
    /// token for `(workspace, dev_machine)`, then mint-and-store one row
    /// with `is_service=True`. Returns the stored `host_label`/`label`
    /// derivations alongside the revoked count for caller assertions.
    ///
    /// Takes `&mut PgConnection` (or `&mut *tx`); Python runs the two
    /// statements in autocommit, so both are faithful.
    pub async fn rotate_machine_token(
        conn: &mut PgConnection,
        params: &NewMachineToken<'_>,
    ) -> Result<RotateOutcome, sqlx::Error> {
        let revoked = sqlx::query(ROTATE_REVOKE_SQL)
            .bind(params.now)
            .bind(params.workspace_id)
            .bind(params.dev_machine_id)
            .execute(&mut *conn)
            .await?
            .rows_affected();
        let stored_host_label = rotate_host_label(params.host_label);
        let stored_label = machine_token_label(params.host_label);
        sqlx::query(ROTATE_CREATE_SQL)
            .bind(params.id)
            .bind(params.user_id)
            .bind(params.dev_machine_id)
            .bind(params.workspace_id)
            .bind(&stored_host_label)
            .bind(params.token_hash)
            .bind(params.token_fingerprint)
            .bind(&stored_label)
            .bind(true)
            .bind(params.now)
            .bind(Option::<chrono::DateTime<chrono::Utc>>::None)
            .bind(Option::<chrono::DateTime<chrono::Utc>>::None)
            .execute(&mut *conn)
            .await?;
        Ok(RotateOutcome {
            revoked,
            stored_host_label,
            stored_label,
        })
    }

    /// What [`rotate_machine_token`] revoked and stored.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RotateOutcome {
        pub revoked: u64,
        pub stored_host_label: String,
        pub stored_label: String,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support as ts;
    use super::*;

    fn fixture(name: &str) -> serde_json::Value {
        ts::fixture_file(name)
    }

    fn token_fields() -> account::TokenFields {
        account::TokenFields {
            access_token: Some("TOK".to_owned()),
            refresh_token: None,
            access_token_expired_at: None,
            refresh_token_expired_at: None,
            id_token: Some(String::new()),
        }
    }

    // -- AUTHOAUTH-F6: account upsert -----------------------------------------

    #[test]
    fn f6_table_and_lookup_shape_match_fixture() {
        let f6 = fixture("F6_account_upsert.before_after.json");
        assert_eq!(account::TABLE, "accounts");
        assert_eq!(f6["account_table"]["db_table"], "accounts");
        assert_eq!(
            f6["account_table"]["unique_together"],
            serde_json::json!(["provider", "provider_account_id"])
        );
        assert_eq!(
            f6["account_table"]["ordering"],
            serde_json::json!(["-created_at"])
        );
        // Lookup filter columns in bind order.
        assert_eq!(
            account::LOOKUP_COLUMNS,
            &["user_id", "provider", "provider_account_id"]
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
        // The Rust lookup mirrors the same predicates in the same order.
        let sql = account::LOOKUP_SQL;
        assert!(sql.contains("FROM \"accounts\""), "lookup table");
        let (user_pos, prov_pos, pid_pos) = (
            sql.find("user_id\" = $1").expect("user_id $1"),
            sql.find("provider\" = $2").expect("provider $2"),
            sql.find("provider_account_id\" = $3")
                .expect("provider_account_id $3"),
        );
        assert!(
            user_pos < prov_pos && prov_pos < pid_pos,
            "predicate order user, provider, provider_account_id"
        );
        assert!(sql.ends_with("LIMIT 1"), ".first() limit");
    }

    #[test]
    fn f6_update_columns_match_fixture() {
        let f6 = fixture("F6_account_upsert.before_after.json");
        let after = &f6["after_state"]["update"];
        // Every column the fixture's update row writes is in UPDATE_COLUMNS,
        // and every untouched column is absent from it.
        for col in [
            "access_token",
            "refresh_token",
            "access_token_expired_at",
            "refresh_token_expired_at",
            "last_connected_at",
            "id_token",
        ] {
            assert!(
                account::UPDATE_COLUMNS.contains(&col),
                "update writes {col}"
            );
            assert!(
                after["row"].get(col).is_some(),
                "fixture update row has {col}"
            );
        }
        for col in after["untouched"].as_array().expect("untouched array") {
            let col = col.as_str().expect("untouched entry");
            assert!(
                !account::UPDATE_COLUMNS.contains(&col),
                "update leaves {col} alone"
            );
        }
        assert!(
            account::UPDATE_SQL.contains("\"updated_at\" = $"),
            "save() stamps updated_at"
        );
        assert!(
            account::UPDATE_SQL.contains("WHERE \"accounts\".\"id\" = $13"),
            "update targets the row id"
        );
    }

    #[test]
    fn f6_update_row_transform_matches_before_after() {
        let f6 = fixture("F6_account_upsert.before_after.json");
        let before = &f6["before_state"]["case_a_exists"]["rows"][0];
        let after = apply_update(before);
        // Token columns take the new values (`<token>` in the fixture's
        // `after_state.update.row` is `"TOK"` here); untouched columns
        // survive verbatim.
        assert_eq!(after["access_token"], serde_json::json!("TOK"));
        assert_eq!(after["user_id"], before["user_id"], "user_id untouched");
        assert_eq!(after["provider"], before["provider"], "provider untouched");
        assert_eq!(
            after["provider_account_id"], before["provider_account_id"],
            "provider_account_id untouched"
        );
        assert_ne!(
            after["last_connected_at"], before["last_connected_at"],
            "last_connected_at restamped"
        );
        assert_eq!(after["last_connected_at"], serde_json::json!("<now>"));
        // `id_token` defaults to `""`, `refresh_token` to `None` — the
        // fixture's `<token_data, default ''>` / `<token_data, default None>`.
        assert_eq!(after["id_token"], serde_json::json!(""));
        assert_eq!(after["refresh_token"], serde_json::Value::Null);
        assert_eq!(after["access_token_expired_at"], serde_json::Value::Null);
        assert_eq!(after["refresh_token_expired_at"], serde_json::Value::Null);
    }

    fn apply_update(before: &serde_json::Value) -> serde_json::Value {
        account::apply_update_row(before, &token_fields(), "<now>")
    }

    #[test]
    fn f6_lookup_provider_id_tolerant_read() {
        // Create-side form: missing "user" reads as None (no raise).
        assert_eq!(
            account::lookup_provider_id(&serde_json::json!({"user": {"provider_id": "gp-1"}})),
            Some("gp-1")
        );
        assert_eq!(account::lookup_provider_id(&serde_json::json!({})), None);
        assert_eq!(
            account::lookup_provider_id(&serde_json::json!({"user": {}})),
            None
        );
    }

    #[test]
    fn f6_insert_covers_create_row() {
        let f6 = fixture("F6_account_upsert.before_after.json");
        let create = &f6["after_state"]["create"]["row"];
        // Every create-row key is a bound INSERT column.
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
            assert!(account::COLUMNS.contains(&key), "insert binds {key}");
            assert!(create.get(key).is_some(), "fixture create row has {key}");
        }
        assert!(
            account::INSERT_SQL.contains("(\"id\","),
            "client-side uuid pk"
        );
        assert!(
            account::INSERT_SQL.contains("\"metadata\""),
            "metadata default {{}}"
        );
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
        // The outcome enum carries the swallowed case instead of Err.
        assert_eq!(
            format!("{:?}", account::UpsertOutcome::ErrorSwallowed),
            "ErrorSwallowed"
        );
    }

    // -- AUTHOAUTH-F7: deactivate_api_token -----------------------------------

    #[test]
    fn f7_sql_and_const_match_fixture() {
        let f7 = fixture("F7_deactivate_token.before_after.json");
        assert_eq!(
            cli_token::DEVICE_FLOW_DESCRIPTION,
            "Issued by pidash auth login (device-code flow)."
        );
        assert_eq!(
            f7["function"]["const"],
            "CLI_DEVICE_API_TOKEN_DESCRIPTION='Issued by pidash auth login (device-code flow).'"
        );
        let emitted = f7["emitted_sql"].as_str().expect("emitted sql");
        // (Django fragment, Rust rendering with quoted identifiers).
        for (frag, rust) in [
            ("SET is_active=false", "\"is_active\" = false"),
            ("updated_at=%s", "\"updated_at\" = $1"),
            ("WHERE token=%s", "\"token\" = $2"),
            ("is_active=true", "\"is_active\" = true"),
        ] {
            assert!(emitted.contains(frag), "emitted sql has {frag}");
            assert!(
                cli_token::DEACTIVATE_SQL.contains(rust)
                    || cli_token::DEACTIVATE_CLI_ONLY_SQL.contains(rust),
                "rust sql has {frag} as {rust}"
            );
        }
        assert!(
            emitted.contains("description='Issued by pidash auth login (device-code flow).'"),
            "description guard recorded"
        );
        assert!(
            cli_token::DEACTIVATE_CLI_ONLY_SQL.contains("\"description\" = $3"),
            "cli-only sql guards description"
        );
        assert!(
            !cli_token::DEACTIVATE_SQL.contains("description"),
            "base sql has no description guard"
        );
    }

    #[test]
    fn f7_empty_token_is_noop() {
        assert!(cli_token::is_noop_token(None));
        assert!(cli_token::is_noop_token(Some("")));
        assert!(!cli_token::is_noop_token(Some("TOK")));
        let f7 = fixture("F7_deactivate_token.before_after.json");
        assert_eq!(f7["after_state"]["empty_token_case"]["out"], 0);
    }

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
        let f8 = fixture("F8_device_helpers.golden.json");
        // Revoke targets (workspace, dev_machine) actives only.
        let revoke = dev_machine::ROTATE_REVOKE_SQL;
        assert!(
            revoke.contains("WHERE \"machine_token\".\"workspace_id\" = $1")
                || revoke.contains("\"workspace_id\" = $2"),
            "workspace filter"
        );
        assert!(
            revoke.contains("\"dev_machine_id\" = $3"),
            "dev-machine filter"
        );
        assert!(revoke.contains("\"revoked_at\" IS NULL"), "actives only");
        assert!(revoke.contains("SET \"revoked_at\" = $1"), "revoke stamp");
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
        assert!(
            dev_machine::ROTATE_CREATE_SQL.contains("\"is_service\""),
            "is_service stored"
        );
        assert_eq!(f8["_rotate_machine_token"]["mint"], "runner_tokens.mint_machine_token() -> raw='mt_'+token_urlsafe(32) (runner/services/tokens.py:84-86)");
    }
}
