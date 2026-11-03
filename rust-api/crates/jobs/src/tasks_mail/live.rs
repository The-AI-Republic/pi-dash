//! Live mail seams for the D-07 send/stack tasks (PIDASHCONV-865).
//!
//! [`email_notification`](super::email_notification) ports the task bodies
//! against domain traits; this module provides the production
//! implementations the worker binary builds in `worker()`:
//!
//! - [`LiveRedisMail`]: the send lock (`SET key "true" NX EX 300`) plus
//!   the `base_api` GET, over the shared async `redis` client (same
//!   features as `pidash-db`, TLS included). The trait methods are sync,
//!   so each call bridges with [`block_on`].
//! - [`SqlMailDb`]: receiver/actor/issue reads, the 7-tuple email
//!   configuration (via the existing
//!   [`get_email_configuration`](pidash_services::license::config::get_email_configuration)
//!   composition — never a fork), the `sent_at` write, and the
//!   per-mention display-name lookup, over the primary pool.
//! - [`IssueUpdatesRenderer`]: the real `issue-updates.html` over
//!   minijinja with Django-compat shims (see below).
//! - [`SmtpMailer`]: the [`SmtpEnvelope`](super::email_notification::SmtpEnvelope)
//!   over the shared [`smtp_send`](super::mail_send::smtp_send)
//!   conversation (same wire as the auth tasks — never a fork).
//! - [`QueueStackOutbox`]: the stack fan-out lands in `rust_job_queue`
//!   (`.delay()` semantics), so post-switchover sends execute on the
//!   Rust worker instead of being published over AMQP for Python.
//!
//! Renderer compatibility (all probe-verified against Django 4.2.30; the
//! vectors live in `fixtures/tasks_mail/send/F-SEND.minijinja-rendered.json`):
//!
//! - `UndefinedBehavior::Chainable` reproduces Django's missing-variable
//!   rules: print as `""`, `{% if %}` as false, `{% for %}` as empty,
//!   chained lookups as undefined, and `undefined > 1` / `undefined ==
//!   'x'` as false (minijinja orders `Undefined` below every kind, so the
//!   never-sent `total_updates`/`total_comments` conditions evaluate
//!   exactly like Django's caught `TypeError`). `Strict` cannot render
//!   this template (`{% if update.changes.target_date %}` on a partial
//!   update would fail), and injecting the missing keys as `""` would
//!   flip `"" > 1` to true (minijinja orders `String` above `Number`) —
//!   so nothing is injected and nothing is strict.
//! - The autoescape format is `Custom("django")` with [`django_formatter`]:
//!   Django's exact `&<>"'` map (notably WITHOUT minijinja's extra `/` →
//!   `&#x2f;`), `None`/`True`/`False` print spellings, and `|safe`
//!   passthrough for the comment bodies.
//! - The embedded Django source is translated at load: `|slice:":2"` →
//!   the [`dj_slice`] compat filter (Python-slice semantics; minijinja's
//!   own `slice` batches instead and cannot parse the colon syntax) and
//!   `|length|add:"-N"` → plain arithmetic. Translated-site counts are
//!   pinned so template drift fails at construction (worker boot), never
//!   silently.
//! - The `length`/`last` builtins are shadowed with Django semantics:
//!   unmeasurable `|length` is `0`; `|last` renders `""` for empty/missing
//!   inputs but fails the render for `None`/mappings/numbers (Django
//!   swallows `IndexError` yet propagates `TypeError`/`KeyError`).
//!
//! Known gap (provably unreachable here): minijinja orders mixed-kind
//! `>`/`<` by kind, so `"" > 1` is true where Django's caught `TypeError`
//! is false. Every ordering comparison in this template meets either an
//! always-undefined `total_*` (false on both sides) or a `|length` number
//! (numeric on both sides) — no `""`, list, or `None` operand ever reaches
//! `>`/`<` — so the matrix pins `{% if x > 1 %}` over undefined only.

use serde_json::{Map, Value};

use super::email_notification::{
    ActorSnapshot, EmailConfig, IssueSnapshot, LockSet, MailDb, MailDbError, MailError, Mailer,
    MentionLookupError, MentionSource, RedisLock, RedisMail, RenderError, Renderer, SmtpEnvelope,
    StackError, StackOutbox,
};
use super::mail_send::{self, OutgoingMail, SmtpSend};
use crate::queue::{self, NewJob};
use crate::task_message::TaskMessage;
use pidash_db::config::ConfigValue;

/// Run an async operation from a sync seam method. The worker runs on the
/// multi-thread runtime, so the current thread is parked aside while the
/// future completes (the `assistant::kms` precedent in `pidash-api`).
/// Panics outside a multi-thread runtime by construction — every caller
/// runs on the worker or on a multi-thread test runtime.
fn block_on<F>(future: F) -> F::Output
where
    F: std::future::Future,
    F::Output: Send,
{
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
}

// ---- LiveRedisMail ----

/// Live [`RedisMail`]: `redis_instance()` (`pi_dash/settings/redis.py`) —
/// the lock `SET`/`DEL` plus the `base_api` GET — over one shared async
/// client (parsing only at construction; each operation multiplexes a
/// connection, the `RedisHandle` shape in `pidash-db`).
#[derive(Debug, Clone)]
pub struct LiveRedisMail {
    client: redis::Client,
}

impl LiveRedisMail {
    /// Parse-only constructor (no I/O; safe in tests and the mirror).
    pub fn from_url(url: &str) -> Result<Self, redis::RedisError> {
        Ok(Self {
            client: pidash_db::redis::open_client(url)?,
        })
    }

    /// Build from resolved settings. A missing/empty/unparsable `REDIS_URL`
    /// is a boot error: the send tasks cannot coordinate without Redis, and
    /// booting anyway would ACK-and-drop mail (the stub failure this issue
    /// exists to prevent). `serve` degrades instead because none of its
    /// handlers own mail traffic.
    pub fn from_settings(settings: &pidash_db::config::Settings) -> Result<Self, String> {
        let url = settings
            .redis
            .url
            .as_deref()
            .filter(|url| !url.is_empty())
            .ok_or_else(|| "REDIS_URL is not set".to_owned())?;
        Self::from_url(url).map_err(|error| format!("REDIS_URL unparsable: {error}"))
    }

    /// `SET key value NX EX ex_secs`: `Some("OK")` on acquire, `None` when
    /// the key already exists (what redis-py returns as `True`/`None`).
    async fn set_nx_ex_async(
        &self,
        key: &str,
        value: &str,
        ex_secs: u64,
    ) -> Result<LockSet, redis::RedisError> {
        use redis::AsyncCommands;
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let options = redis::SetOptions::default()
            .conditional_set(redis::ExistenceCheck::NX)
            .with_expiration(redis::SetExpiry::EX(ex_secs));
        let set: Option<String> = connection.set_options(key, value, options).await?;
        Ok(if set.is_some() {
            LockSet::Acquired
        } else {
            LockSet::Contended
        })
    }

    async fn del_async(&self, key: &str) -> Result<(), redis::RedisError> {
        use redis::AsyncCommands;
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let _: i64 = connection.del(key).await?;
        Ok(())
    }

    /// `GET key`, decoded — `None` when absent. Undecodable bytes are an
    /// `Err`, like redis-py's `.decode()` raising `UnicodeDecodeError`.
    async fn get_async(&self, key: &str) -> Result<Option<String>, redis::RedisError> {
        use redis::AsyncCommands;
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        connection.get(key).await
    }
}

impl RedisLock for LiveRedisMail {
    fn set_nx_ex(&self, key: &str, value: &str, ex_secs: u64) -> LockSet {
        match block_on(self.set_nx_ex_async(key, value, ex_secs)) {
            Ok(set) => set,
            Err(error) => {
                // No error channel on the trait (redis-py raises into the
                // task's outer `except` instead): fail closed — a send that
                // cannot prove it holds the lock must not send — and log
                // the outage (the `log_exception` half of that path).
                tracing::error!(%error, key, "mail redis lock acquire failed; treating as contended");
                LockSet::Contended
            }
        }
    }

    fn del(&self, key: &str) {
        // Python ignores the `delete` return; a transport failure here is
        // logged, never raised (the release runs on success paths too).
        if let Err(error) = block_on(self.del_async(key)) {
            tracing::warn!(%error, key, "mail redis lock release failed");
        }
    }
}

impl RedisMail for LiveRedisMail {
    fn get(&self, key: &str) -> Option<String> {
        match block_on(self.get_async(key)) {
            Ok(value) => value,
            Err(error) => {
                // redis-py raises (connection errors and `UnicodeDecodeError`
                // alike) into the outer `except`; without an error channel
                // this degrades to the base_api-missing early return, with
                // the outage logged. Micro-divergence, documented: Python
                // releases the lock on that path while this return holds it
                // until expiry — reachable only when Redis serves
                // undecodable bytes or dies between the lock and the GET.
                tracing::error!(%error, key, "mail redis GET failed; treating base_api as missing");
                None
            }
        }
    }
}

// ---- SqlMailDb ----

/// `User.objects.get(pk=receiver_id)` projected to `receiver.email` (`:183`,
/// read at `:251`, `:280`): the `email` column is nullable.
const MAIL_RECEIVER_SQL: &str = "SELECT email FROM users WHERE id = $1";

/// `User.objects.get(pk=actor_id)` (`:191`) projected to the template
/// fields, with the `avatar_url` property's inputs in one round trip: the
/// `avatar` text, the `avatar_asset_id` FK, and the asset row's dispatch
/// inputs (`entity_type`, `project_id`, `issue_id`, workspace slug).
/// `users` has no `deleted_at` column (stock `UserManager`); the asset and
/// workspace joins are scoped to live rows like the FK descriptors they
/// replace (a soft-deleted asset/workspace raises `DoesNotExist` in
/// Python — the generic `except`, i.e. [`MailDbError::Db`]).
const MAIL_ACTOR_SQL: &str = "SELECT u.first_name, u.last_name, u.avatar, u.avatar_asset_id, \
     a.id AS asset_id, a.entity_type, a.project_id, a.issue_id, w.slug AS workspace_slug \
     FROM users u LEFT JOIN file_assets a ON a.id = u.avatar_asset_id AND a.deleted_at IS NULL \
     LEFT JOIN workspaces w ON w.id = a.workspace_id AND w.deleted_at IS NULL WHERE u.id = $1";

/// `Issue.objects.get(pk=issue_id)` (`:184`): the issue columns,
/// scoped to live rows (`SoftDeletionManager` — a soft-deleted issue is
/// `DoesNotExist`, the silent path); the project/workspace joins below
/// mirror the lazy `issue.project` / `issue.project.workspace` fetches (a
/// missing join row is a generic `except`, never the silent
/// `DoesNotExist` path).
const MAIL_ISSUE_SQL: &str =
    "SELECT id, name, sequence_id, project_id FROM issues WHERE id = $1 AND deleted_at IS NULL";

/// `issue.project` + `issue.project.workspace` (`:236-259`), scoped to
/// live rows like the descriptors (soft-deleted on either side is the
/// generic `except`).
const MAIL_PROJECT_SQL: &str = "SELECT p.id, p.identifier, p.name, w.slug \
     FROM projects p JOIN workspaces w ON w.id = p.workspace_id \
     WHERE p.id = $1 AND p.deleted_at IS NULL AND w.deleted_at IS NULL";

/// `EmailNotificationLog.objects.filter(pk__in=email_notification_ids)
/// .update(sent_at=timezone.now())` (`:287`): a no-op for an empty id list,
/// exactly like the ORM call — and soft-deleted rows stay unmarked, like
/// the manager-scoped queryset.
const MAIL_SENT_SQL: &str =
    "UPDATE email_notification_logs SET sent_at = now() WHERE id = ANY($1) AND deleted_at IS NULL";

/// Per-mention `User.objects.get(pk=user_id).display_name` (`:136`).
const MAIL_DISPLAY_NAME_SQL: &str = "SELECT display_name FROM users WHERE id = $1";

/// Live [`MailDb`] over the primary pool. Reads go to the primary: a
/// background task has no request context to opt into the replica, and the
/// rows it marks seconds later must be the rows it read (the `SqlStackStore`
/// precedent).
#[derive(Debug, Clone)]
pub struct SqlMailDb {
    pool: sqlx::PgPool,
}

impl SqlMailDb {
    /// Borrow the primary pool handle (cloned — `PgPool` is cheap to clone).
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

/// Parse a wire id: `User`/`Issue.objects.get(pk="garbage")` raises
/// `ValidationError`, which is NOT `(Issue|User).DoesNotExist` — the outer
/// generic `except`, i.e. [`MailDbError::Db`] (logged release + return).
fn parse_id(raw: &str) -> Result<uuid::Uuid, MailDbError> {
    raw.parse::<uuid::Uuid>()
        .map_err(|_| MailDbError::Db(format!("bad uuid {raw:?}")))
}

/// `User.avatar_url` (`db/models/user.py:143-154`) over one actor row: the
/// asset URL when `avatar_asset_id` is set (which may itself be `None`),
/// else the `avatar` text when non-empty, else `None`. The asset branch
/// runs the full `FileAsset.asset_url` dispatch
/// (`db/models/asset.py:80-100`): static-asset ids format directly;
/// attachment/description branches need the workspace slug (a null slug is
/// `RelatedObjectDoesNotExist` — a [`MailDbError::Db`]), while null
/// `project_id`/`issue_id` render as the four letters `None` (raw f-string
/// values, never fetched); any other entity type (null included) is `None`.
fn avatar_url_for_row(
    avatar: &str,
    avatar_asset_id: Option<uuid::Uuid>,
    asset_id: Option<uuid::Uuid>,
    entity_type: Option<&str>,
    project_id: Option<uuid::Uuid>,
    issue_id: Option<uuid::Uuid>,
    workspace_slug: Option<&str>,
) -> Result<Option<String>, MailDbError> {
    let Some(wanted) = avatar_asset_id else {
        return Ok(if avatar.is_empty() {
            None
        } else {
            Some(avatar.to_owned())
        });
    };
    let found =
        asset_id.ok_or_else(|| MailDbError::Db(format!("dangling avatar asset {wanted}")))?;
    match entity_type {
        Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
            Ok(Some(format!("/api/assets/v2/static/{found}/")))
        }
        Some("ISSUE_ATTACHMENT") => {
            let slug = workspace_slug
                .ok_or_else(|| MailDbError::Db(format!("avatar asset {found} has no workspace")))?;
            Ok(Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{}/issues/{}/attachments/{found}/",
                project_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "None".to_owned()),
                issue_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "None".to_owned()),
            )))
        }
        Some(
            "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION",
        ) => {
            let slug = workspace_slug
                .ok_or_else(|| MailDbError::Db(format!("avatar asset {found} has no workspace")))?;
            Ok(Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{}/{found}/",
                project_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "None".to_owned()),
            )))
        }
        _ => Ok(None),
    }
}

/// Map the 7-tuple to [`EmailConfig`]: host/user/pass keep the
/// `resolve_smtp` string coercion (every env read and DB row is a string;
/// `Null` stays absent); port and the TLS/SSL flags stay raw `Value`s for
/// the mid-send `int()` / `=="1"` parses; a null/empty sender falls back to
/// Django's `DEFAULT_FROM_EMAIL` (the `mail_send::resolve_smtp` mapping,
/// verified against the interpreter there).
fn email_config_from_values(values: &[ConfigValue]) -> Result<EmailConfig, MailDbError> {
    if values.len() != 7 {
        return Err(MailDbError::Db(format!(
            "expected 7 email values, got {}",
            values.len()
        )));
    }
    let string_of = |v: &ConfigValue| match v {
        ConfigValue::Str(s) => Some(s.clone()),
        ConfigValue::Int(i) => Some(i.to_string()),
        ConfigValue::Float(f) => Some(f.to_string()),
        ConfigValue::Bool(true) => Some("True".to_owned()),
        ConfigValue::Bool(false) => Some("False".to_owned()),
        ConfigValue::Null => None,
    };
    let value_of = |v: &ConfigValue| match v {
        ConfigValue::Str(s) => Value::String(s.clone()),
        ConfigValue::Int(i) => Value::Number(serde_json::Number::from(*i)),
        ConfigValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ConfigValue::Bool(b) => Value::Bool(*b),
        ConfigValue::Null => Value::Null,
    };
    let from_email = match string_of(&values[6]) {
        Some(from) if !from.is_empty() => from,
        _ => mail_send::DEFAULT_FROM_EMAIL.to_owned(),
    };
    Ok(EmailConfig {
        host: string_of(&values[0]),
        username: string_of(&values[1]),
        password: string_of(&values[2]),
        port: value_of(&values[3]),
        use_tls: value_of(&values[4]),
        use_ssl: value_of(&values[5]),
        from_email,
    })
}

impl MailDb for SqlMailDb {
    async fn receiver_email(&self, receiver_id: &str) -> Result<Option<String>, MailDbError> {
        use sqlx::Row;
        let id = parse_id(receiver_id)?;
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(MAIL_RECEIVER_SQL)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        match row {
            None => Err(MailDbError::NotFound(receiver_id.to_owned())),
            Some(row) => row
                .try_get("email")
                .map_err(|error| MailDbError::Db(error.to_string())),
        }
    }

    async fn actor(&self, actor_id: &str) -> Result<ActorSnapshot, MailDbError> {
        use sqlx::Row;
        let id = parse_id(actor_id)?;
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(MAIL_ACTOR_SQL)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let row = row.ok_or_else(|| MailDbError::NotFound(actor_id.to_owned()))?;
        let first_name: String = row
            .try_get("first_name")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let last_name: String = row
            .try_get("last_name")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let avatar: String = row
            .try_get("avatar")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let avatar_asset_id: Option<uuid::Uuid> = row
            .try_get("avatar_asset_id")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let asset_id: Option<uuid::Uuid> = row
            .try_get("asset_id")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let entity_type: Option<String> = row
            .try_get("entity_type")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let project_id: Option<uuid::Uuid> = row
            .try_get("project_id")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let issue_id: Option<uuid::Uuid> = row
            .try_get("issue_id")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let workspace_slug: Option<String> = row
            .try_get("workspace_slug")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        Ok(ActorSnapshot {
            first_name,
            last_name,
            avatar_url: avatar_url_for_row(
                &avatar,
                avatar_asset_id,
                asset_id,
                entity_type.as_deref(),
                project_id,
                issue_id,
                workspace_slug.as_deref(),
            )?,
        })
    }

    async fn issue(&self, issue_id: Option<&str>) -> Result<IssueSnapshot, MailDbError> {
        use sqlx::Row;
        // The null-`entity_identifier` stack row: `Issue.objects.get(pk=None)`
        // raises `DoesNotExist` — silent release + return, no query needed.
        let raw = issue_id.ok_or_else(|| MailDbError::NotFound("None".to_owned()))?;
        let id = parse_id(raw)?;
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(MAIL_ISSUE_SQL)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let row = row.ok_or_else(|| MailDbError::NotFound(raw.to_owned()))?;
        let issue_id: uuid::Uuid = row
            .try_get("id")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let name: String = row
            .try_get("name")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let sequence_id: i32 = row
            .try_get("sequence_id")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let project_id: uuid::Uuid = row
            .try_get("project_id")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        // The lazy joins: `Project`/`Workspace.DoesNotExist` is NOT in the
        // task's silent tuple — the generic `except`, i.e. `Db`.
        let joined: Option<sqlx::postgres::PgRow> = sqlx::query(MAIL_PROJECT_SQL)
            .bind(project_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let joined =
            joined.ok_or_else(|| MailDbError::Db(format!("issue {raw} has no project")))?;
        let project_id: uuid::Uuid = joined
            .try_get("id")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let identifier: String = joined
            .try_get("identifier")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let project_name: String = joined
            .try_get("name")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        let workspace_slug: String = joined
            .try_get("slug")
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        Ok(IssueSnapshot {
            id: issue_id.to_string(),
            name,
            identifier,
            sequence_id: i64::from(sequence_id),
            project_id: project_id.to_string(),
            project_name,
            workspace_slug,
        })
    }

    async fn email_config(&self) -> Result<EmailConfig, MailDbError> {
        // `get_email_configuration()` runs INSIDE the send flow in Python
        // (`:173-181`): a store failure is a generic-`except` logged
        // release, like every other read error here.
        let store = pidash_db::config::PgConfigStore::new(self.pool.clone());
        let keyring = pidash_db::config::encryption::Keyring::from_env();
        let registry = pidash_db::config::registry::global();
        let values =
            pidash_services::license::config::get_email_configuration(&store, &keyring, registry)
                .await
                .map_err(|error| MailDbError::Db(format!("email configuration failed: {error}")))?;
        email_config_from_values(&values)
    }

    async fn mark_sent(&self, email_notification_ids: &[String]) -> Result<(), MailDbError> {
        // All-or-nothing like the ORM: `filter(pk__in=[...])` validates
        // every id before touching a row, so parse first, then execute.
        let mut ids = Vec::with_capacity(email_notification_ids.len());
        for raw in email_notification_ids {
            ids.push(parse_id(raw)?);
        }
        sqlx::query(MAIL_SENT_SQL)
            .bind(ids)
            .execute(&self.pool)
            .await
            .map_err(|error| MailDbError::Db(error.to_string()))?;
        Ok(())
    }
}

impl MentionSource for SqlMailDb {
    async fn display_name(&self, user_id: &str) -> Result<String, MentionLookupError> {
        use sqlx::Row;
        let id = user_id
            .parse::<uuid::Uuid>()
            .map_err(|_| MentionLookupError::Db(format!("bad uuid {user_id:?}")))?;
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(MAIL_DISPLAY_NAME_SQL)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| MentionLookupError::Db(error.to_string()))?;
        match row {
            None => Err(MentionLookupError::NotFound(user_id.to_owned())),
            Some(row) => row
                .try_get("display_name")
                .map_err(|error| MentionLookupError::Db(error.to_string())),
        }
    }
}

// ---- IssueUpdatesRenderer ----

/// The Django original, embedded so template edits flow into the next build
/// (same `include_str!` precedent as the auth templates in `mail_send`).
const ISSUE_UPDATES_SOURCE: &str =
    include_str!("../../../../../apps/api/templates/emails/notifications/issue-updates.html");

/// Translated-site pins: `|slice:":2"` × 4, `|length|add:"-2"` × 4,
/// `|length|add:"-1"` × 4. Any other count is template drift.
const EXPECTED_SLICE_SITES: usize = 4;
const EXPECTED_ADD2_SITES: usize = 4;
const EXPECTED_ADD1_SITES: usize = 4;

/// Replace every `needle` with `replacement`, reporting the site count.
fn replace_all(haystack: &str, needle: &str, replacement: &str) -> (String, usize) {
    let count = haystack.matches(needle).count();
    (haystack.replace(needle, replacement), count)
}

/// Translate the embedded Django source to minijinja-parseable syntax: the
/// colon-argument filters Django allows (`|slice:":2"`, `|add:"-N"`) are a
/// parse error in minijinja, so `|slice:":2"` becomes the [`dj_slice`]
/// compat filter and `|length|add:"-N"` becomes plain arithmetic (the filter
/// binds tighter than `-`, so the grouping is preserved). Any leftover
/// colon-filter syntax or a drifted site count fails construction — at
/// worker boot, never silently at send time.
fn translate_template(source: &str) -> Result<String, RenderError> {
    let (translated, slice_sites) = replace_all(source, "|slice:\":2\"", "|dj_slice(\":2\")");
    let (translated, add2_sites) = replace_all(&translated, "|length|add:\"-2\"", "|length - 2");
    let (translated, add1_sites) = replace_all(&translated, "|length|add:\"-1\"", "|length - 1");
    if translated.contains("|slice:") || translated.contains("|add:") {
        return Err(RenderError::Failed(
            "untranslated Django filter syntax left in template".to_owned(),
        ));
    }
    if (slice_sites, add2_sites, add1_sites)
        != (
            EXPECTED_SLICE_SITES,
            EXPECTED_ADD2_SITES,
            EXPECTED_ADD1_SITES,
        )
    {
        return Err(RenderError::Failed(format!(
            "template drift: expected {EXPECTED_SLICE_SITES}/{EXPECTED_ADD2_SITES}/{EXPECTED_ADD1_SITES} \
             translated sites, got {slice_sites}/{add2_sites}/{add1_sites}"
        )));
    }
    Ok(translated)
}

/// Parse one Python-slice bound: empty is open, else a Python `int()`.
fn slice_bound(text: &str) -> Option<Option<i64>> {
    if text.is_empty() {
        return Some(None);
    }
    super::mail_send::parse_py_int(text).map(Some)
}

/// Django `|slice:"spec"` (`defaultfilters.slice`): `value[slice(start,
/// stop, step)]` with `str(arg).split(":")` bounds (fewer than 3 pads with
/// open; more than 3, a bad bound, or a zero step fails). Sequences slice
/// by item and strings by char; anything else (including `None`) comes back
/// unchanged — Django catches `(ValueError, TypeError)` — except mappings,
/// whose `dict[slice]` raises an uncaught `KeyError` (all probe-verified).
fn dj_slice(
    value: minijinja::value::Value,
    spec: String,
) -> Result<minijinja::value::Value, minijinja::Error> {
    use minijinja::value::{Value as MiniValue, ValueKind};
    let mut bits: Vec<Option<i64>> = Vec::new();
    for part in spec.split(':') {
        match slice_bound(part) {
            Some(bound) => bits.push(bound),
            None => return Ok(value),
        }
    }
    if bits.len() > 3 {
        return Ok(value);
    }
    while bits.len() < 3 {
        bits.push(None);
    }
    let step = bits[2].unwrap_or(1);
    if step == 0 {
        return Ok(value);
    }
    match value.kind() {
        ValueKind::Seq => {
            let items: Vec<MiniValue> = value
                .try_iter()
                .map(|iter| iter.collect())
                .unwrap_or_default();
            Ok(MiniValue::from(slice_items(&items, bits[0], bits[1], step)))
        }
        ValueKind::String => {
            let chars: Vec<char> = value.as_str().unwrap_or_default().chars().collect();
            Ok(MiniValue::from(
                slice_items(&chars, bits[0], bits[1], step)
                    .into_iter()
                    .collect::<String>(),
            ))
        }
        ValueKind::Map => Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "cannot slice a mapping",
        )),
        _ => Ok(value),
    }
}

/// Python `seq[start:stop:step]` over a slice (`PySlice_AdjustIndices`
/// rules): negative bounds count from the end; a forward step clamps to
/// `[0, len]` while a backward step clamps into `[-1, len - 1]` (the `-1`
/// floor is what makes `[:-99:-1]` walk the whole sequence); a negative
/// step walks down from `start` (defaulting to the last item) while above
/// `stop` (defaulting past the first item).
fn slice_items<T: Clone>(items: &[T], start: Option<i64>, stop: Option<i64>, step: i64) -> Vec<T> {
    let len = items.len() as i64;
    let mut out = Vec::new();
    if step > 0 {
        let resolve = |bound: Option<i64>, default: i64| match bound {
            None => default,
            Some(index) if index < 0 => (len + index).clamp(0, len),
            Some(index) => index.clamp(0, len),
        };
        let mut index = resolve(start, 0);
        let end = resolve(stop, len);
        while index < end {
            out.push(items[index as usize].clone());
            index += step;
        }
    } else {
        let resolve = |bound: Option<i64>, default: i64| match bound {
            None => default,
            Some(index) if index < 0 => {
                let adjusted = len + index;
                if adjusted < 0 {
                    -1
                } else {
                    adjusted.min(len - 1)
                }
            }
            Some(index) => index.min(len - 1),
        };
        let mut index = resolve(start, len - 1);
        let end = resolve(stop, -1);
        while index > end {
            out.push(items[index as usize].clone());
            index += step;
        }
    }
    out
}

/// Django `|length` (`defaultfilters.length`): `len()` with
/// `(ValueError, TypeError)` → `0`. Shadows the builtin, which errors on
/// unmeasurable values instead (the `link.old_value|length` guard hits a
/// missing key on links without removals).
fn dj_length(value: &minijinja::value::Value) -> usize {
    if let Some(text) = value.as_str() {
        return text.chars().count();
    }
    value.len().unwrap_or(0)
}

/// Django `|last` (`defaultfilters.last`): bare `value[-1]` — an empty
/// sequence/string, or a missing key (which resolves to `""` before the
/// filter runs), renders `""`, while `None`/mappings/numbers fail the
/// render (probe-verified 4.2 behavior: `IndexError` is swallowed,
/// `TypeError`/`KeyError` propagate). Shadows the builtin, whose empty
/// and error shapes differ.
fn dj_last(value: &minijinja::value::Value) -> Result<minijinja::value::Value, minijinja::Error> {
    use minijinja::value::{Value as MiniValue, ValueKind};
    match value.kind() {
        ValueKind::Undefined => Ok(MiniValue::from("")),
        ValueKind::String => Ok(value
            .as_str()
            .and_then(|text| text.chars().next_back())
            .map(|ch| MiniValue::from(ch.to_string()))
            .unwrap_or_else(|| MiniValue::from(""))),
        ValueKind::Seq => {
            let mut last = None;
            if let Ok(iter) = value.try_iter() {
                for item in iter {
                    last = Some(item);
                }
            }
            Ok(last.unwrap_or_else(|| MiniValue::from("")))
        }
        _ => Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "cannot get last item from value",
        )),
    }
}

/// Django autoescape as a minijinja formatter (`Custom("django")`): safe
/// strings (the `|safe` comment bodies) pass through raw, exactly like
/// minijinja's own `write_escaped`; every other value renders with
/// `str()`-then-escape — `None`/`True`/`False` spellings included — over
/// the exact `&<>"'` map (notably WITHOUT the `/` → `&#x2f;` minijinja's
/// own escaper adds, which would corrupt every URL in the mail).
fn django_formatter(
    out: &mut minijinja::Output,
    state: &minijinja::State,
    value: &minijinja::value::Value,
) -> Result<(), minijinja::Error> {
    debug_assert!(matches!(
        state.auto_escape(),
        minijinja::AutoEscape::Custom("django")
    ));
    if value.is_safe() {
        return out
            .write_str(value.as_str().unwrap_or_default())
            .map_err(minijinja::Error::from);
    }
    let text = match value.kind() {
        minijinja::value::ValueKind::Undefined => String::new(),
        minijinja::value::ValueKind::None => "None".to_owned(),
        minijinja::value::ValueKind::Bool => {
            if value.is_true() {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        minijinja::value::ValueKind::Number => value.to_string(),
        _ => mail_send::django_escape(&value.to_string()),
    };
    out.write_str(&text).map_err(minijinja::Error::from)
}

/// Convert one JSON context value into a minijinja value by hand, never
/// through serde.
///
/// `template.render(&Map<String, serde_json::Value>)` serializes the
/// context via serde — and the workspace unifies
/// `serde_json/arbitrary_precision` (from `pidash-api`), under which
/// `Number::serialize` emits the internal token struct instead of a
/// u64/i64/f64. minijinja then sees every JSON number as a map:
/// `|length` counts its keys and `total > 1` compares by kind order
/// (PIDASHCONV-865 D1: green under `-p pidash-jobs`, red under
/// `--workspace`, and the shipped worker links the same unified
/// features). Walking the value here keeps numbers numeric however the
/// crate graph unifies; rendering takes the converted value, whose own
/// `Serialize` round-trips through minijinja's value-handle fast path.
fn mini_value(value: &Value) -> minijinja::value::Value {
    use minijinja::value::Value as MiniValue;
    match value {
        Value::Null => MiniValue::from(()),
        Value::Bool(flag) => MiniValue::from(*flag),
        Value::Number(number) => mini_number(number),
        Value::String(text) => MiniValue::from(text.clone()),
        Value::Array(items) => items.iter().map(mini_value).collect::<MiniValue>(),
        Value::Object(map) => map
            .iter()
            .map(|(key, item)| (key.clone(), mini_value(item)))
            .collect::<MiniValue>(),
    }
}

/// A JSON number as the narrowest minijinja number. `as_u64`/`as_i64`
/// parse the stored token, so they stay exact under
/// `arbitrary_precision`; only an out-of-range integer falls through to
/// the 128-bit parse and then to its decimal spelling (unreachable in
/// mail contexts — counts and small ids — but must not panic; as a
/// display string it still renders the same digits).
fn mini_number(number: &serde_json::Number) -> minijinja::value::Value {
    use minijinja::value::Value as MiniValue;
    if let Some(int) = number.as_u64() {
        MiniValue::from(int)
    } else if let Some(int) = number.as_i64() {
        MiniValue::from(int)
    } else if number.is_f64() {
        // Checked above, so the parse cannot fail; `is_f64` (not
        // `as_f64().is_some()`) keeps out-of-range integers on the exact
        // 128-bit path instead of a lossy float.
        MiniValue::from(number.as_f64().expect("is_f64 checked"))
    } else if let Ok(int) = number.to_string().parse::<u128>() {
        MiniValue::from(int)
    } else if let Ok(int) = number.to_string().parse::<i128>() {
        MiniValue::from(int)
    } else {
        MiniValue::from(number.to_string())
    }
}

/// Live [`Renderer`]: the real `issue-updates.html` over minijinja with
/// the Django-compat shims above. The environment is built once (template
/// drift fails construction, at worker boot) and renders many contexts.
pub struct IssueUpdatesRenderer {
    env: minijinja::Environment<'static>,
}

impl IssueUpdatesRenderer {
    /// Build the rendering environment: translate + parse the embedded
    /// template, install `Chainable` undefined handling, the Django escape
    /// format, and the `length`/`last`/`dj_slice` filters.
    pub fn new() -> Result<Self, RenderError> {
        let mut env = minijinja::Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Chainable);
        env.set_auto_escape_callback(|_| minijinja::AutoEscape::Custom("django"));
        env.set_formatter(django_formatter);
        // Django keeps the template file's trailing newline; minijinja
        // strips it by default (the `mail_send` precedent — drift-safe if
        // the template ever gains one).
        env.set_keep_trailing_newline(true);
        env.add_filter("length", dj_length);
        env.add_filter("last", dj_last);
        env.add_filter("dj_slice", dj_slice);
        let source = translate_template(ISSUE_UPDATES_SOURCE)?;
        env.add_template_owned("issue-updates.html", source)
            .map_err(|error| RenderError::Failed(format!("bad template: {error}")))?;
        Ok(Self { env })
    }

    /// Render an ad-hoc template with the live environment (tests only:
    /// pins shim behavior against Django outputs).
    #[cfg(test)]
    fn render_str(
        &self,
        source: &str,
        context: &Map<String, Value>,
    ) -> Result<String, RenderError> {
        let converted = mini_value(&Value::Object(context.clone()));
        self.env
            .render_str(source, &converted)
            .map_err(|error| RenderError::Failed(error.to_string()))
    }
}

impl Renderer for IssueUpdatesRenderer {
    fn render_issue_updates(&self, context: &Map<String, Value>) -> Result<String, RenderError> {
        let template = self
            .env
            .get_template("issue-updates.html")
            .map_err(|error| RenderError::Failed(format!("bad template: {error}")))?;
        // Convert by hand (see `mini_value`): rendering the JSON map
        // directly would serialize numbers through serde.
        let converted = mini_value(&Value::Object(context.clone()));
        template
            .render(&converted)
            .map_err(|error| RenderError::Failed(format!("render failed: {error}")))
    }
}

// ---- SmtpMailer ----

/// Live [`Mailer`]: the [`SmtpEnvelope`] over the shared
/// [`smtp_send`](mail_send::smtp_send) conversation (greeting, EHLO, STARTTLS,
/// AUTH, `MAIL`/`RCPT`/`DATA` — the same wire the auth tasks use, never a
/// fork), bridged onto the async runtime. Stateless: the envelope carries
/// the resolved configuration.
#[derive(Debug, Clone, Copy, Default)]
pub struct SmtpMailer;

impl SmtpMailer {
    /// Build the stateless sender.
    pub fn new() -> Self {
        Self
    }
}

/// Build the shared-pipeline inputs from one envelope: a null/empty host
/// connects to `localhost` (Django's `EMAIL_HOST` default — the
/// `resolve_smtp` mapping); a null recipient fails the send (Python raises
/// inside the send-block `try`).
fn smtp_inputs(envelope: &SmtpEnvelope) -> Result<(SmtpSend, OutgoingMail), MailError> {
    let to = envelope
        .to
        .clone()
        .ok_or_else(|| MailError::Failed("missing recipient".to_owned()))?;
    let config = SmtpSend {
        host: match envelope.host.as_deref() {
            Some(host) if !host.is_empty() => host.to_owned(),
            _ => mail_send::DEFAULT_SMTP_HOST.to_owned(),
        },
        port: envelope.port,
        username: envelope.username.clone(),
        password: envelope.password.clone(),
        use_tls: envelope.use_tls,
        use_ssl: envelope.use_ssl,
        from_email: envelope.from_email.clone(),
    };
    let mail = OutgoingMail {
        to,
        subject: envelope.subject.clone(),
        text_body: envelope.text_content.clone(),
        html_body: envelope.html_content.clone(),
    };
    Ok((config, mail))
}

impl Mailer for SmtpMailer {
    fn send(&self, envelope: &SmtpEnvelope) -> Result<(), MailError> {
        let (config, mail) = smtp_inputs(envelope)?;
        block_on(mail_send::smtp_send(&config, &mail)).map_err(MailError::Failed)
    }
}

// ---- QueueStackOutbox ----

/// Live [`StackOutbox`]: the fan-out lands in `rust_job_queue` (`.delay()`
/// semantics: default queue, immediate, default retries — with the
/// `pg_notify` push-wake the shared [`enqueue`](queue::enqueue) carries), so
/// post-switchover sends execute on the Rust worker instead of being
/// published over AMQP for the Python plane.
#[derive(Debug, Clone)]
pub struct QueueStackOutbox {
    pool: sqlx::PgPool,
}

impl QueueStackOutbox {
    /// Borrow the primary pool handle (cloned — `PgPool` is cheap to clone).
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

/// `.delay()` as a queue row: task name plus wire args/kwargs verbatim.
fn delay_job(message: &TaskMessage) -> NewJob {
    NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    )
}

impl StackOutbox for QueueStackOutbox {
    async fn delay_send(&self, message: TaskMessage) -> Result<(), StackError> {
        queue::enqueue(&self.pool, &delay_job(&message))
            .await
            .map_err(|error| StackError::Outbox(error.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const GOLDEN: &str =
        include_str!("../../../../fixtures/tasks_mail/send/F-SEND.minijinja-rendered.json");

    fn golden_doc() -> Value {
        serde_json::from_str(GOLDEN).expect("golden parses")
    }

    fn golden_case(doc: &Value, name: &str) -> (Map<String, Value>, String) {
        let case = doc.get(name).unwrap_or(&Value::Null);
        let context = case
            .get("context")
            .and_then(|context| context.as_object())
            .cloned()
            .expect("context");
        let html = case
            .get("html")
            .and_then(|html| html.as_str())
            .expect("html")
            .to_owned();
        (context, html)
    }

    #[test]
    fn json_numbers_convert_to_mini_numbers_not_maps() {
        // D1: under serde_json/arbitrary_precision (unified workspace-wide
        // via pidash-api) `Number::serialize` emits the token struct, so a
        // serde-built context hands minijinja maps instead of numbers
        // (`{{ x|length }}` renders "1", `total > 1` goes plural). The
        // manual conversion must produce real numbers however the crate
        // graph unifies — pin every JSON scalar plus nesting.
        use minijinja::value::ValueKind;
        assert_eq!(mini_value(&Value::Null).kind(), ValueKind::None);
        assert_eq!(mini_value(&json!(true)).kind(), ValueKind::Bool);
        assert_eq!(mini_value(&json!("5")).kind(), ValueKind::String);
        for number in [json!(5), json!(-7), json!(1.5), json!(0)] {
            let converted = mini_value(&number);
            assert_eq!(converted.kind(), ValueKind::Number, "{number}");
            assert_eq!(converted.to_string(), number.to_string());
        }
        let nested = mini_value(&json!({"n": 5, "items": [1, {"m": 2}]}));
        assert_eq!(nested.kind(), ValueKind::Map);
        let renderer = IssueUpdatesRenderer::new().expect("env builds");
        let mut context = Map::new();
        context.insert("x".to_owned(), json!(5));
        assert_eq!(
            renderer.render_str("{{ x|length }}", &context),
            Ok("0".to_owned())
        );
    }

    #[test]
    fn renders_multi_actor_golden_byte_for_byte() {
        let doc = golden_doc();
        let (context, expected) = golden_case(&doc, "multi");
        let renderer = IssueUpdatesRenderer::new().expect("env builds");
        let html = renderer.render_issue_updates(&context).expect("renders");
        assert_eq!(html, expected);
    }

    #[test]
    fn renders_single_actor_golden_byte_for_byte() {
        let doc = golden_doc();
        let (context, expected) = golden_case(&doc, "single");
        let renderer = IssueUpdatesRenderer::new().expect("env builds");
        let html = renderer.render_issue_updates(&context).expect("renders");
        assert_eq!(html, expected);
    }

    #[test]
    fn filter_matrix_matches_django() {
        let doc = golden_doc();
        let renderer = IssueUpdatesRenderer::new().expect("env builds");
        let filters = doc
            .get("filters")
            .and_then(|filters| filters.as_object())
            .expect("filters");
        assert!(!filters.is_empty(), "matrix must not be empty");
        for (name, case) in filters {
            let template = case
                .get("mini_template")
                .and_then(|template| template.as_str())
                .expect("mini_template");
            let context = case
                .get("context")
                .and_then(|context| context.as_object())
                .expect("context");
            let expected = case
                .get("output")
                .and_then(|output| output.as_str())
                .expect("output");
            let rendered = renderer.render_str(template, context);
            if let Some(django_error) = expected.strip_prefix("ERROR:") {
                assert!(
                    rendered.is_err(),
                    "{name}: Django fails ({django_error}) but minijinja renders {rendered:?}"
                );
            } else {
                assert_eq!(
                    rendered.as_deref(),
                    Ok(expected),
                    "{name} (django: {})",
                    case.get("django_template")
                        .and_then(|template| template.as_str())
                        .unwrap_or("?")
                );
            }
        }
    }

    #[test]
    fn translate_rejects_leftover_django_syntax() {
        assert!(translate_template("{{ x|slice:\":5\" }}").is_err());
        assert!(translate_template("{{ x|add:\"1\" }}").is_err());
    }

    #[test]
    fn translate_pins_site_counts() {
        assert!(translate_template("no filters here").is_err());
        assert!(translate_template(ISSUE_UPDATES_SOURCE).is_ok());
    }

    #[test]
    fn slice_items_follows_python_semantics() {
        let items = ["a", "b", "c", "d"];
        assert_eq!(slice_items(&items, None, Some(2), 1), ["a", "b"]);
        assert_eq!(slice_items(&items, Some(1), None, 2), ["b", "d"]);
        assert_eq!(slice_items(&items, Some(-2), None, 1), ["c", "d"]);
        assert_eq!(slice_items(&items, None, None, -1), ["d", "c", "b", "a"]);
        assert_eq!(slice_items(&items, Some(99), None, 1), Vec::<&str>::new());
        assert_eq!(
            slice_items(&items, None, Some(-99), -1),
            ["d", "c", "b", "a"]
        );
        assert!(slice_items(&items, Some(-99), None, -1).is_empty());
        let empty: [&str; 0] = [];
        assert_eq!(slice_items(&empty, None, Some(2), 1), Vec::<&str>::new());
        assert_eq!(slice_items(&empty, None, None, -1), Vec::<&str>::new());
    }

    const TEST_REDIS_URL: &str = "redis://127.0.0.1:6379/";

    fn test_redis() -> LiveRedisMail {
        LiveRedisMail::from_url(TEST_REDIS_URL).expect("url parses")
    }

    fn test_key() -> String {
        format!("pidashconv-865-test-{}", uuid::Uuid::new_v4())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn redis_lock_acquire_contend_release_roundtrip() {
        // Needs a server on 127.0.0.1:6379 — the `redis` service in the
        // rust-api workflows (same assumption as pidash-db's redis tests).
        let mail = test_redis();
        let key = test_key();
        assert_eq!(mail.set_nx_ex(&key, "true", 300), LockSet::Acquired);
        assert_eq!(mail.set_nx_ex(&key, "true", 300), LockSet::Contended);
        mail.del(&key);
        assert_eq!(mail.set_nx_ex(&key, "true", 300), LockSet::Acquired);
        mail.del(&key);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn redis_get_hit_and_miss() {
        let mail = test_redis();
        let key = test_key();
        assert_eq!(mail.get(&key), None);
        assert_eq!(
            mail.set_nx_ex(&key, "http://localhost", 300),
            LockSet::Acquired
        );
        assert_eq!(mail.get(&key).as_deref(), Some("http://localhost"));
        mail.del(&key);
        assert_eq!(mail.get(&key), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn redis_outage_fails_closed_with_no_panic() {
        // Refused port: the bridge must surface the failure (contended /
        // missing), never hang or panic.
        let dead = LiveRedisMail::from_url("redis://127.0.0.1:1/").expect("url parses");
        assert_eq!(
            dead.set_nx_ex("pidashconv-865-test-dead", "true", 300),
            LockSet::Contended
        );
        assert_eq!(dead.get("pidashconv-865-test-dead"), None);
        dead.del("pidashconv-865-test-dead");
    }

    #[test]
    fn redis_from_settings_requires_url() {
        let settings = pidash_db::config::Settings::test_defaults();
        assert!(settings.redis.url.is_none());
        assert!(LiveRedisMail::from_settings(&settings).is_err());
        let mut bad = settings.clone();
        bad.redis.url = Some(":// not a url".to_owned());
        assert!(LiveRedisMail::from_settings(&bad).is_err());
        let mut good = settings;
        good.redis.url = Some(TEST_REDIS_URL.to_owned());
        assert!(LiveRedisMail::from_settings(&good).is_ok());
    }

    fn lazy_pool() -> sqlx::PgPool {
        sqlx::PgPool::connect_lazy("postgres://127.0.0.1:1/nope").expect("lazy")
    }

    #[tokio::test]
    async fn mail_statements_parse_as_postgres() {
        use sqlparser::parser::Parser;
        for sql in [
            MAIL_RECEIVER_SQL,
            MAIL_ACTOR_SQL,
            MAIL_ISSUE_SQL,
            MAIL_PROJECT_SQL,
            MAIL_SENT_SQL,
            MAIL_DISPLAY_NAME_SQL,
        ] {
            Parser::parse_sql(&sqlparser::dialect::PostgreSqlDialect {}, sql).expect("parses");
        }
    }

    #[test]
    fn mail_statements_scope_soft_deleted_rows() {
        // The default managers scope every soft-deletable read
        // (`SoftDeletionManager`); `users` has no `deleted_at` column, so
        // only its two statements stay unscoped (the `pg_notify` pin
        // precedent in `queue.rs`).
        for sql in [
            MAIL_ACTOR_SQL,
            MAIL_ISSUE_SQL,
            MAIL_PROJECT_SQL,
            MAIL_SENT_SQL,
        ] {
            assert!(
                sql.contains("deleted_at IS NULL"),
                "unscoped soft-deletable read: {sql}"
            );
        }
        for sql in [MAIL_RECEIVER_SQL, MAIL_DISPLAY_NAME_SQL] {
            assert!(
                !sql.contains("deleted_at"),
                "users has no deleted_at column: {sql}"
            );
        }
    }

    #[tokio::test]
    async fn garbage_ids_map_to_db_not_notfound() {
        // No database is touched: the UUID parse fails first (lazy pool).
        // `ValidationError` is the generic `except`, never the silent path.
        let db = SqlMailDb::new(lazy_pool());
        assert!(matches!(
            db.receiver_email("not-a-uuid").await,
            Err(MailDbError::Db(_))
        ));
        assert!(matches!(
            db.actor("not-a-uuid").await,
            Err(MailDbError::Db(_))
        ));
        assert!(matches!(
            db.issue(Some("not-a-uuid")).await,
            Err(MailDbError::Db(_))
        ));
        assert!(matches!(
            db.mark_sent(&["not-a-uuid".to_owned()]).await,
            Err(MailDbError::Db(_))
        ));
        assert!(matches!(
            db.display_name("not-a-uuid").await,
            Err(MentionLookupError::Db(_))
        ));
        // The null-issue row short-circuits to NotFound without a query.
        assert!(matches!(
            db.issue(None).await,
            Err(MailDbError::NotFound(_))
        ));
    }

    #[test]
    fn avatar_url_matches_property_matrix() {
        let asset = uuid::Uuid::new_v4();
        let static_url = format!("/api/assets/v2/static/{asset}/");
        // No asset: the avatar text wins unless empty.
        assert_eq!(
            avatar_url_for_row("http://x/a.png", None, None, None, None, None, None).unwrap(),
            Some("http://x/a.png".to_owned())
        );
        assert_eq!(
            avatar_url_for_row("", None, None, None, None, None, None).unwrap(),
            None
        );
        // Static-asset entity types format directly (avatar text ignored).
        for entity in [
            "WORKSPACE_LOGO",
            "USER_AVATAR",
            "USER_COVER",
            "PROJECT_COVER",
        ] {
            assert_eq!(
                avatar_url_for_row(
                    "ignored",
                    Some(asset),
                    Some(asset),
                    Some(entity),
                    None,
                    None,
                    None
                )
                .unwrap(),
                Some(static_url.clone()),
                "{entity}"
            );
        }
        // Attachment branch joins the workspace slug; null FKs render None.
        assert_eq!(
            avatar_url_for_row(
                "",
                Some(asset),
                Some(asset),
                Some("ISSUE_ATTACHMENT"),
                None,
                None,
                Some("acme")
            )
            .unwrap(),
            Some(format!(
                "/api/assets/v2/workspaces/acme/projects/None/issues/None/attachments/{asset}/"
            ))
        );
        // Description branch.
        assert_eq!(
            avatar_url_for_row(
                "",
                Some(asset),
                Some(asset),
                Some("ISSUE_DESCRIPTION"),
                None,
                None,
                Some("acme")
            )
            .unwrap(),
            Some(format!(
                "/api/assets/v2/workspaces/acme/projects/None/{asset}/"
            ))
        );
        // Unknown/null entity type is None, never the avatar text.
        assert_eq!(
            avatar_url_for_row(
                "ignored",
                Some(asset),
                Some(asset),
                Some("NOPE"),
                None,
                None,
                None
            )
            .unwrap(),
            None
        );
        assert_eq!(
            avatar_url_for_row("ignored", Some(asset), Some(asset), None, None, None, None)
                .unwrap(),
            None
        );
        // Dangling asset / missing slug are store errors.
        assert!(avatar_url_for_row("", Some(asset), None, None, None, None, None).is_err());
        assert!(avatar_url_for_row(
            "",
            Some(asset),
            Some(asset),
            Some("ISSUE_ATTACHMENT"),
            None,
            None,
            None
        )
        .is_err());
    }

    #[test]
    fn email_config_mapping_matches_resolve_smtp() {
        use pidash_db::config::ConfigValue::{Bool, Int, Null, Str};
        let values = vec![
            Str("smtp.test".to_owned()),
            Str("user".to_owned()),
            Str("pass".to_owned()),
            Str("587".to_owned()),
            Str("1".to_owned()),
            Str("0".to_owned()),
            Str("Team <t@e.com>".to_owned()),
        ];
        let config = email_config_from_values(&values).expect("maps");
        assert_eq!(config.host.as_deref(), Some("smtp.test"));
        assert_eq!(config.username.as_deref(), Some("user"));
        assert_eq!(config.port, json!("587"));
        assert_eq!(config.use_tls, json!("1"));
        assert_eq!(config.use_ssl, json!("0"));
        assert_eq!(config.from_email, "Team <t@e.com>");
        // Nulls: host stays absent, the int port default survives raw, the
        // sender falls back to Django's default.
        let values = vec![
            Null,
            Null,
            Null,
            Int(587),
            Str("1".to_owned()),
            Str("0".to_owned()),
            Null,
        ];
        let config = email_config_from_values(&values).expect("maps");
        assert_eq!(config.host, None);
        assert_eq!(config.port, json!(587));
        assert_eq!(config.from_email, mail_send::DEFAULT_FROM_EMAIL);
        // An empty sender falls back too; a bool port stays raw for the
        // mid-send `int()` parse to reject.
        let values = vec![
            Str(String::new()),
            Bool(true),
            Null,
            Bool(true),
            Int(1),
            Null,
            Str(String::new()),
        ];
        let config = email_config_from_values(&values).expect("maps");
        assert_eq!(config.host.as_deref(), Some(""));
        assert_eq!(config.username.as_deref(), Some("True"));
        assert_eq!(config.port, json!(true));
        assert_eq!(config.use_tls, json!(1));
        assert_eq!(config.use_ssl, Value::Null);
        assert_eq!(config.from_email, mail_send::DEFAULT_FROM_EMAIL);
        // Wrong arity fails.
        assert!(email_config_from_values(&values[..6]).is_err());
    }

    fn test_envelope() -> SmtpEnvelope {
        SmtpEnvelope {
            host: Some("smtp.test".to_owned()),
            port: 587,
            username: Some("u".to_owned()),
            password: Some("p".to_owned()),
            use_tls: true,
            use_ssl: false,
            from_email: "Team <t@e.com>".to_owned(),
            to: Some("to@e.com".to_owned()),
            subject: "s".to_owned(),
            text_content: "t".to_owned(),
            html_content: "<p>t</p>".to_owned(),
        }
    }

    #[test]
    fn smtp_inputs_applies_django_fallbacks() {
        let (config, mail) = smtp_inputs(&test_envelope()).expect("maps");
        assert_eq!(config.host, "smtp.test");
        assert_eq!(config.port, 587);
        assert!(config.use_tls);
        assert_eq!(mail.to, "to@e.com");
        let mut envelope = test_envelope();
        envelope.host = None;
        assert_eq!(
            smtp_inputs(&envelope).expect("maps").0.host,
            mail_send::DEFAULT_SMTP_HOST
        );
        envelope.host = Some(String::new());
        assert_eq!(
            smtp_inputs(&envelope).expect("maps").0.host,
            mail_send::DEFAULT_SMTP_HOST
        );
        envelope.to = None;
        assert!(smtp_inputs(&envelope).is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn mailer_send_propagates_transport_errors() {
        // Refused port: the bridge must surface the failure, never hang.
        let mut envelope = test_envelope();
        envelope.host = Some("127.0.0.1".to_owned());
        envelope.port = 1;
        assert!(SmtpMailer::new().send(&envelope).is_err());
        // Mapping failures never touch the network.
        envelope.to = None;
        assert!(SmtpMailer::new().send(&envelope).is_err());
    }

    #[test]
    fn delay_job_carries_wire_parts_verbatim() {
        let mut kwargs = Map::new();
        kwargs.insert("issue_id".to_owned(), Value::Null);
        let message = TaskMessage::new("task.name", vec![Value::String("a".to_owned())], kwargs);
        let job = delay_job(&message);
        assert_eq!(job.task, "task.name");
        assert_eq!(job.args, json!(["a"]));
        assert!(job.kwargs.get("issue_id").is_some_and(|id| id.is_null()));
        assert_eq!(job.queue, queue::DEFAULT_QUEUE);
        assert!(job.visible_at.is_none());
        assert_eq!(job.max_retries, queue::DEFAULT_MAX_RETRIES);
    }
}
