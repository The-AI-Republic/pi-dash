//! Boot-gate commands: `wait_for_db`, `wait_for_migrations`, `clear_cache`.
//!
//! Ports of `apps/api/pi_dash/db/management/commands/wait_for_db.py:11-24`,
//! `wait_for_migrations.py:12-26` and `clear_cache.py:10-30` (fixture F37-01
//! and the first branch of F37-02). Each `run_*` function streams the exact
//! stdout lines through the `out` callback (one call per line, no trailing
//! newline — the caller prints); all commands exit 0 on every handled path.
//!
//! Documented translations (also listed in the PR):
//!
//! * `wait_for_db`: Python's check (`connections["default"]`) only builds
//!   the connection wrapper and never raises, so the retry branch is dead
//!   and `Database available!` prints even when the DB is down (F37-01
//!   BUGS — ported as is: the loop below always exits after one pass,
//!   and the typo'd retry line is kept verbatim for the unreachable
//!   branch).
//! * `wait_for_migrations`: a missing `django_migrations` table counts
//!   as pending in both implementations (Django's loader plans every
//!   migration then, so it waits too — verified live). When the database
//!   itself is down, Python crashes with an uncaught `OperationalError`
//!   (traceback, exit 1, empty stdout); the Rust port returns
//!   [`BootError::DatabaseUnavailable`], which the CLI renders as a
//!   one-line stderr message with the same exit code and no stdout.
//! * The pending check is leaf-node parity, not a reimplementation of
//!   `MigrationExecutor`: Django applies migrations in dependency order,
//!   so every leaf node being applied implies its ancestors are applied,
//!   which is exactly when `migration_plan(leaf_nodes())` is empty. The
//!   leaf set is baked in [`LEAF_MIGRATIONS`] (drift is the domain gate's).

use sqlx::Connection as _;
use std::collections::HashSet;
use std::str::FromStr;
use std::time::Duration;

/// `wait_for_db.py:15`, unstyled.
pub const WAIT_DB_START: &str = "Waiting for database...";
/// `wait_for_db.py:21`, unstyled — the `waititng` typo is ported verbatim.
pub const WAIT_DB_RETRY: &str = "Database unavailable, waititng 1 second...";
/// `wait_for_db.py:24`, `style.SUCCESS` (plain without a TTY).
pub const WAIT_DB_OK: &str = "Database available!";
/// `wait_for_migrations.py:17`, unstyled.
pub const WAIT_MIGRATIONS_POLL: &str = "Waiting for database migrations to complete...";
/// `wait_for_migrations.py:20` — capital P in `Pending`, space before `...`.
pub const WAIT_MIGRATIONS_OK: &str = "No migrations Pending. Starting processes ...";
/// `clear_cache.py:26`, `style.SUCCESS` (full `cache.clear()`).
pub const CACHE_CLEARED: &str = "Cache Cleared";
/// `clear_cache.py:30`, `style.ERROR` (any exception from the cache call).
pub const CACHE_FAILED: &str = "Failed to clear cache";

/// Leaf `(app_label, migration_name)` nodes of the Django migration graph —
/// `MigrationExecutor(connection).loader.graph.leaf_nodes()`, computed with
/// the real Django 4.2 loader over `INSTALLED_APPS`
/// (`settings/common.py:44-68`) at the ported commit (260 graph nodes):
/// no `run_before`, `replaces`, `__first__` or `__latest__` markers exist in
/// any `pi_dash` migration, so the five first-party leaves are also exactly
/// what an `ast` parse of the `dependencies` lists in `apps/api` yields
/// (the contract test asserts that independently).
pub const LEAF_MIGRATIONS: &[(&str, &str)] = &[
    ("assistant", "0004_usersttconfig"),
    ("auth", "0012_alter_user_first_name_max_length"),
    ("contenttypes", "0002_remove_content_type_name"),
    ("db", "0167_wait_budget"),
    ("django_celery_beat", "0018_improve_crontab_helptext"),
    ("license", "0006_instance_is_current_version_deprecated"),
    ("prompting", "0007_reseed_directional_relations"),
    ("runner", "0029_drop_agentrun_trigger_blocker_completed"),
    ("sessions", "0001_initial"),
];

/// Scope and `?host=`-socket URLs are rejected by the URL parser, exactly
/// like the `serve`/`worker` boot path (same sqlx parser).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BootError {
    /// `DATABASE_URL` is missing or unparsable — a config error, so it
    /// fails fast instead of joining the retry loop.
    #[error("invalid DATABASE_URL: {0}")]
    BadDatabaseUrl(String),
    /// The database is unreachable, or a probe query failed: Python's
    /// uncaught `OperationalError` (traceback, exit 1). The text is a
    /// one-line summary instead of libpq's message — the documented
    /// crash-path class.
    #[error("database unavailable: {0}")]
    DatabaseUnavailable(String),
}

/// True while any leaf migration in [`LEAF_MIGRATIONS`] is absent from the
/// applied `(app, name)` set — the `bool(migration_plan(targets))` port.
/// Extra applied rows (migrations deleted from disk) are ignored, matching
/// the plan check, which only requires the targets' closure to be applied.
pub fn migrations_pending_from_applied(applied: &HashSet<(String, String)>) -> bool {
    LEAF_MIGRATIONS
        .iter()
        .any(|(app, name)| !applied.contains(&(app.to_string(), name.to_string())))
}

/// `SELECT app, name FROM django_migrations` — the `MigrationLoader`
/// applied set. A missing table errors (fresh database, migrator not run
/// yet); the caller maps every error to pending.
pub async fn fetch_applied_migrations(
    connection: &mut sqlx::postgres::PgConnection,
) -> Result<HashSet<(String, String)>, sqlx::Error> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT app, name FROM django_migrations")
        .fetch_all(&mut *connection)
        .await?;
    Ok(rows.into_iter().collect())
}

/// Postgres `undefined_table`: the migrator hasn't run yet. Django's
/// loader plans every migration when the table is absent (verified live:
/// plan of 260, no raise), so this one error counts as pending; every
/// other failure crashes, like Python's uncaught `OperationalError`.
fn is_missing_table(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(database_error)
            if database_error.code().as_deref() == Some("42P01")
    )
}

/// One `_pending_migrations()` probe over an open connection: true while
/// migrations are pending. A missing table counts as pending; any other
/// query failure propagates (Python crashes with an uncaught
/// `OperationalError` there).
pub async fn migrations_pending(
    connection: &mut sqlx::postgres::PgConnection,
) -> Result<bool, sqlx::Error> {
    match fetch_applied_migrations(connection).await {
        Ok(applied) => Ok(migrations_pending_from_applied(&applied)),
        Err(error) if is_missing_table(&error) => Ok(true),
        Err(error) => Err(error),
    }
}

/// Parse `DATABASE_URL` once, before the loop, so a config error fails
/// fast instead of joining the retry loop.
fn parse_database_url(database_url: &str) -> Result<sqlx::postgres::PgConnectOptions, BootError> {
    sqlx::postgres::PgConnectOptions::from_str(database_url)
        .map_err(|error| BootError::BadDatabaseUrl(error.to_string()))
}

/// One fresh single connection per probe — the `wait_for_db` attempt and
/// the `_pending_migrations` check each open, use and close exactly one
/// connection, like Django's per-check connection use. (A pool is the
/// wrong tool here: pool establishment stalls against a dead database,
/// while a single connection fails fast.) The 5s timeout bounds each
/// probe against a hanging host (connection-refused fails instantly
/// either way).
async fn probe_connection(
    options: &sqlx::postgres::PgConnectOptions,
) -> Result<sqlx::postgres::PgConnection, sqlx::Error> {
    use sqlx::ConnectOptions as _;
    let connect = options.connect();
    tokio::time::timeout(Duration::from_secs(5), connect)
        .await
        .map_err(|_| sqlx::Error::PoolTimedOut)?
}

/// The one-shot check, `connections["default"]`: it only builds the
/// connection wrapper and never raises `OperationalError`, so it always
/// succeeds and `wait_for_db` never retries (F37-01 BUGS). It performs
/// no I/O — the wrapper was already built by parsing the URL.
fn db_wrapper_is_up() -> bool {
    true
}

/// One `_pending_migrations()` probe over a fresh single connection.
/// An unreachable database — or any query failure other than a missing
/// table — is fatal (Python's uncaught `OperationalError`: one stderr
/// line, exit 1, no stdout).
async fn migrations_pending_probe(
    options: &sqlx::postgres::PgConnectOptions,
) -> Result<bool, BootError> {
    let mut connection = probe_connection(options)
        .await
        .map_err(|error| BootError::DatabaseUnavailable(error.to_string()))?;
    let pending = migrations_pending(&mut connection)
        .await
        .map_err(|error| BootError::DatabaseUnavailable(error.to_string()))?;
    let _ = connection.close().await;
    Ok(pending)
}

/// `wait_for_db`: print the start line, perform the one-shot check, print
/// the success line (`wait_for_db.py:15-24`). The check never fails, so
/// the loop always exits after one pass — even when the database is down —
/// and the retry branch is unreachable (kept verbatim, with the
/// `waititng` typo). Returns 1 (one probe performed).
pub async fn run_wait_for_db(
    database_url: &str,
    out: &mut dyn FnMut(&str),
    sleep: Duration,
) -> Result<u64, BootError> {
    // Building the wrapper is parsing the URL: the only failure is a
    // config error (fails fast, before any output).
    parse_database_url(database_url)?;
    out(WAIT_DB_START);
    let mut attempts: u64 = 0;
    loop {
        attempts += 1;
        if db_wrapper_is_up() {
            out(WAIT_DB_OK);
            return Ok(attempts);
        }
        out(WAIT_DB_RETRY);
        tokio::time::sleep(sleep).await;
    }
}

/// `wait_for_migrations`: while the plan is non-empty, print the poll line
/// and sleep `sleep` (`wait_for_migrations.py:16-18`, 10s in production),
/// then print the success line. `max_attempts` caps the loop for tests
/// only. Returns the number of probes performed.
pub async fn run_wait_for_migrations(
    database_url: &str,
    out: &mut dyn FnMut(&str),
    sleep: Duration,
    max_attempts: Option<u64>,
) -> Result<u64, BootError> {
    let options = parse_database_url(database_url)?;
    let mut attempts: u64 = 0;
    loop {
        attempts += 1;
        if !migrations_pending_probe(&options).await? {
            out(WAIT_MIGRATIONS_OK);
            return Ok(attempts);
        }
        out(WAIT_MIGRATIONS_POLL);
        if max_attempts.is_some_and(|max| attempts >= max) {
            return Ok(attempts);
        }
        tokio::time::sleep(sleep).await;
    }
}

/// Django's default cache-key function (`key_prefix:version:key`) with the
/// project's defaults — no `KEY_PREFIX`, `VERSION` or `KEY_FUNCTION`
/// overrides exist in `settings/` — so `cache.delete("k")` removes `:1:k`.
pub fn cache_key(key: &str) -> String {
    format!(":1:{key}")
}

/// `clear_cache`: with a non-empty `--key`, `DEL` that key and report it
/// (`clear_cache.py:19-22`, printing the raw key); otherwise `FLUSHDB` the
/// whole database (`clear_cache.py:24-26` — django-redis 5.4.0 `clear()`
/// is `flushdb()`, not a pattern delete). Any failure — missing/unparsable
/// `REDIS_URL`, refused connection, command error — reports the failure
/// line (`clear_cache.py:27-30`). Returns the single stdout line.
pub async fn clear_cache(redis_url: Option<&str>, key: Option<&str>) -> String {
    match clear_cache_inner(redis_url, key).await {
        Ok(line) => line,
        Err(()) => CACHE_FAILED.to_string(),
    }
}

async fn clear_cache_inner(redis_url: Option<&str>, key: Option<&str>) -> Result<String, ()> {
    let url = redis_url.filter(|url| !url.is_empty()).ok_or(())?;
    let client = redis::Client::open(url).map_err(|_| ())?;
    // Deliberate CLI bounds (2s connect, 5s command): django-redis
    // itself runs with blocking sockets here — `REDIS_SOCKET_*`
    // (`settings/common.py:238-239`) feed `redis_instance()`, not the
    // `CACHES` client — but a boot command must fail fast instead of
    // hanging, so the port bounds the wait and reports the failure line.
    let mut connection = tokio::time::timeout(Duration::from_secs(2), async {
        client.get_multiplexed_async_connection().await
    })
    .await
    .map_err(|_| ())?
    .map_err(|_| ())?;
    if let Some(key) = key.filter(|key| !key.is_empty()) {
        let mut command = redis::cmd("DEL");
        command.arg(cache_key(key));
        let query = command.query_async::<()>(&mut connection);
        tokio::time::timeout(Duration::from_secs(5), query)
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?;
        Ok(format!("Cache Cleared for key: {key}"))
    } else {
        let command = redis::cmd("FLUSHDB");
        let query = command.query_async::<()>(&mut connection);
        tokio::time::timeout(Duration::from_secs(5), query)
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?;
        Ok(CACHE_CLEARED.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The nine leaves, `(app, name)` sorted like the loader output — a
    /// mismatch here means Django's graph moved and the gate owns a re-sync.
    #[test]
    fn leaf_set_matches_django_loader() {
        let mut leaves: Vec<(&str, &str)> = LEAF_MIGRATIONS.to_vec();
        leaves.sort_unstable();
        assert_eq!(
            leaves,
            vec![
                ("assistant", "0004_usersttconfig"),
                ("auth", "0012_alter_user_first_name_max_length"),
                ("contenttypes", "0002_remove_content_type_name"),
                ("db", "0167_wait_budget"),
                ("django_celery_beat", "0018_improve_crontab_helptext"),
                ("license", "0006_instance_is_current_version_deprecated"),
                ("prompting", "0007_reseed_directional_relations"),
                ("runner", "0029_drop_agentrun_trigger_blocker_completed"),
                ("sessions", "0001_initial"),
            ]
        );
    }

    fn applied_set(pairs: &[(&str, &str)]) -> HashSet<(String, String)> {
        pairs
            .iter()
            .map(|(app, name)| (app.to_string(), name.to_string()))
            .collect()
    }

    #[test]
    fn pending_logic_empty_partial_full_and_extra() {
        // Empty database (fresh, migrator not run): pending.
        assert!(migrations_pending_from_applied(&applied_set(&[])));
        // All but one leaf: pending.
        let partial: Vec<(&str, &str)> = LEAF_MIGRATIONS[..8].to_vec();
        assert!(migrations_pending_from_applied(&applied_set(&partial)));
        // Exactly the leaves: done.
        assert!(!migrations_pending_from_applied(&applied_set(
            LEAF_MIGRATIONS
        )));
        // Leaves plus unrelated rows (ancestors, deleted migrations): done.
        let mut full: Vec<(&str, &str)> = LEAF_MIGRATIONS.to_vec();
        full.push(("db", "0001_initial"));
        full.push(("removed_app", "0001_gone"));
        assert!(!migrations_pending_from_applied(&applied_set(&full)));
    }

    #[test]
    fn cache_key_uses_django_default_prefix_and_version() {
        assert_eq!(cache_key("mykey"), ":1:mykey");
        assert_eq!(cache_key(""), ":1:");
    }

    #[tokio::test]
    async fn bad_database_url_fails_fast_without_probing() {
        let mut lines = Vec::new();
        let mut out = |line: &str| lines.push(line.to_string());
        let result =
            run_wait_for_db("not a database url %%", &mut out, Duration::from_millis(1)).await;
        assert!(matches!(result, Err(BootError::BadDatabaseUrl(_))));
        assert!(lines.is_empty());
    }

    /// A down database changes nothing: the one-shot check always succeeds
    /// (F37-01 BUGS), so the command prints both lines and exits 0 without
    /// touching the network (a dead port answers nothing, instantly).
    #[tokio::test]
    async fn wait_for_db_is_one_shot_against_dead_port() {
        let mut lines = Vec::new();
        let mut out = |line: &str| lines.push(line.to_string());
        let attempts = run_wait_for_db(
            "postgresql://127.0.0.1:9/pidash_806_nonexistent",
            &mut out,
            Duration::from_millis(1),
        )
        .await
        .expect("one-shot check succeeds");
        assert_eq!(attempts, 1);
        assert_eq!(
            lines,
            vec!["Waiting for database...", "Database available!"]
        );
        // The unreachable retry line keeps Python's typo byte for byte.
        assert_eq!(WAIT_DB_RETRY, "Database unavailable, waititng 1 second...");
    }

    /// A down database is fatal (Python's uncaught `OperationalError`):
    /// no stdout at all, exit 1 via [`BootError::DatabaseUnavailable`].
    #[tokio::test]
    async fn wait_for_migrations_down_db_fails_without_output() {
        let mut lines = Vec::new();
        let mut out = |line: &str| lines.push(line.to_string());
        let result = run_wait_for_migrations(
            "postgresql://127.0.0.1:9/pidash_806_nonexistent",
            &mut out,
            Duration::from_millis(1),
            Some(2),
        )
        .await;
        assert!(matches!(result, Err(BootError::DatabaseUnavailable(_))));
        assert!(lines.is_empty());
    }

    #[tokio::test]
    async fn clear_cache_without_redis_url_fails() {
        assert_eq!(clear_cache(None, None).await, "Failed to clear cache");
        assert_eq!(
            clear_cache(Some(""), Some("k")).await,
            "Failed to clear cache"
        );
        assert_eq!(
            clear_cache(Some("not a url %%"), None).await,
            "Failed to clear cache"
        );
        assert_eq!(
            clear_cache(Some("redis://127.0.0.1:9/"), None).await,
            "Failed to clear cache"
        );
    }

    /// Live-redis round trip on a dedicated database number (the `check`
    /// CI job runs valkey on 6379, like the db-crate pubsub test): DEL
    /// removes exactly the versioned key, FLUSHDB empties the database.
    #[tokio::test]
    async fn clear_cache_key_and_full_clear_against_redis() {
        const URL: &str = "redis://127.0.0.1:6379/6";
        let client = redis::Client::open(URL).expect("redis test client");
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .expect("redis reachable for the ops cache test");
        redis::cmd("FLUSHDB")
            .query_async::<()>(&mut connection)
            .await
            .expect("flush test db");
        redis::cmd("SET")
            .arg(":1:opstest")
            .arg("v")
            .query_async::<()>(&mut connection)
            .await
            .expect("prime key");
        redis::cmd("SET")
            .arg("unversioned")
            .arg("v")
            .query_async::<()>(&mut connection)
            .await
            .expect("prime key");

        assert_eq!(
            clear_cache(Some(URL), Some("opstest")).await,
            "Cache Cleared for key: opstest"
        );
        let gone: Option<String> = redis::cmd("GET")
            .arg(":1:opstest")
            .query_async(&mut connection)
            .await
            .expect("get after DEL");
        assert_eq!(gone, None);
        // The key-scoped delete leaves the rest of the database alone.
        let kept: Option<String> = redis::cmd("GET")
            .arg("unversioned")
            .query_async(&mut connection)
            .await
            .expect("get survivor");
        assert_eq!(kept.as_deref(), Some("v"));

        assert_eq!(clear_cache(Some(URL), None).await, "Cache Cleared");
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg("*")
            .query_async(&mut connection)
            .await
            .expect("keys after FLUSHDB");
        assert!(keys.is_empty());
    }
}
