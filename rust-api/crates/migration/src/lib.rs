#![forbid(unsafe_code)]

//! Native OSS schema migrator (NEWPIDASH-380).
//!
//! The Rust migrator owns the schema. It lives in OSS under `rust-api/`,
//! needs no Python, and handles both fresh installs (a baseline snapshot at
//! the current leaf, stamped with the full `django_migrations` history) and
//! upgrades (forward ports, one transaction per migration, fail-fast with
//! the failing migration named).
//!
//! Modelled on `rust-cloud/crates/cloud-migration` (SeaORM migrator, an
//! advisory lock, a split owner/app database URL): `up` applies OSS
//! migrations in dependency order under one advisory lock, records each
//! Django-equivalent migration in `django_migrations` (so a fallback Django
//! still sees a fully migrated database), and imports migrations Django
//! already applied without rerunning them. `status` and `check` are
//! read-only: they never create tables, never poll, never sleep.
//!
//! The cloud binary builds the same migrator from this OSS migration set
//! plus its own apps' migrations (like `INSTALLED_APPS`); cloud migrations
//! may depend on OSS migrations, never the reverse. See the schema-ownership
//! design (`.ai_design/schema_ownership/design.md`).

mod baseline;
pub mod ensure;

pub use baseline::BASELINE;

use std::collections::HashSet;
use std::time::Duration;

use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, DbErr, Statement,
    TransactionTrait,
};
use sea_orm_migration::{async_trait, MigrationName, MigrationTrait, MigratorTrait, SchemaManager};
use serde::Serialize;

/// SeaORM history table for the OSS migrator. Separate name from the cloud
/// migrator's `pidash_cloud_schema_migrations`: the OSS binary runs OSS
/// migrations only, the cloud binary the combined set, and neither history
/// may be mistaken for the other's.
pub const HISTORY_TABLE: &str = "pidash_schema_migrations";

// Advisory-lock key for the whole `up` run. Stable per migrator: concurrent
// `up` runs serialize on it, and the value must differ from the cloud
// migrator's key (`pidash_cloud_migration::LOCK_ID`, "PIDASHMI") so an OSS
// `up` and a cloud `up` against the same database do not interleave DDL.
pub const LOCK_ID: i64 = 0x5049_4441_5348_4D4F;

/// Forward ports after the baseline: `(django_app, django_name)` pairs in
/// application order. Empty until Part 2 lands the baseline snapshot and the
/// first forwards (`runner/0044`, the `ensure_schema` upgrades); `up` refuses
/// databases that are not at the baseline until then.
pub const FORWARDS: &[(&str, &str)] = &[];

/// Native-only migrations: no Django `(app, name)` history entry is stamped.
/// Ordered after the forward ports. The `ensure_schema` upgrade (boot-time
/// DDL moved into the migration set) is first; runner/0044 follows once
/// NEWPIDASH-372 merges.
pub const NATIVE: &[(&str, &str)] = &[("rust", "0002_ensure_schema_tables")];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub baseline_missing: Vec<String>,
    pub pending: Vec<String>,
    pub imported_from_django: Vec<String>,
}

impl Status {
    pub fn ready(&self) -> bool {
        self.baseline_missing.is_empty() && self.pending.is_empty()
    }

    fn require_ready(&self) -> Result<(), DbErr> {
        if self.ready() {
            Ok(())
        } else {
            let mut missing = self.baseline_missing.clone();
            missing.extend(self.pending.clone());
            Err(DbErr::Custom(format!(
                "Database migrations are pending: {}. Run the release migrator before starting the API.",
                missing.join(", ")
            )))
        }
    }
}

fn sql(query: &str) -> Statement {
    Statement::from_string(DbBackend::Postgres, query)
}

/// The baseline's state on a database: fully migrated by Django (import
/// path), untouched (fresh-install path), or neither (refuse: only an empty
/// database is bootstrapped, and only a baseline database is upgraded).
#[derive(Debug, Clone, PartialEq, Eq)]
enum BaselineState {
    Current,
    Fresh,
    Partial(Vec<String>),
}

async fn baseline_state(db: &impl ConnectionTrait) -> Result<BaselineState, DbErr> {
    if !table_exists(db, "django_migrations").await? {
        return Ok(BaselineState::Fresh);
    }
    let mut applied = HashSet::new();
    for row in db
        .query_all(sql("SELECT app, name FROM django_migrations"))
        .await?
    {
        applied.insert((
            row.try_get::<String>("", "app")?,
            row.try_get::<String>("", "name")?,
        ));
    }
    let missing = BASELINE
        .iter()
        .filter(|(app, name)| !applied.contains(&(app.to_string(), name.to_string())))
        .map(|(app, name)| format!("{app}/{name}"))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(BaselineState::Current)
    } else {
        // Empty-but-present django_migrations is also partial: the snapshot
        // always stamps, so a stamp-less table is not one this migrator
        // built. Refuse rather than snapshotting into existing tables.
        Ok(BaselineState::Partial(missing))
    }
}

async fn table_exists(db: &impl ConnectionTrait, name: &str) -> Result<bool, DbErr> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT to_regclass($1) IS NOT NULL AS present",
            [name.into()],
        ))
        .await?
        .ok_or_else(|| DbErr::Custom("No catalog response".into()))?;
    row.try_get("", "present")
}

/// Read-only: unlike SeaORM's default `status`, never creates a history table.
/// A missing `django_migrations` table means an empty database (the migrator
/// was never run and Django never migrated): every baseline row is missing.
pub async fn status(db: &impl ConnectionTrait) -> Result<Status, DbErr> {
    let mut applied = HashSet::new();
    if table_exists(db, "django_migrations").await? {
        for row in db
            .query_all(sql("SELECT app, name FROM django_migrations"))
            .await?
        {
            applied.insert((
                row.try_get::<String>("", "app")?,
                row.try_get::<String>("", "name")?,
            ));
        }
    }
    let mut native = HashSet::new();
    if table_exists(db, HISTORY_TABLE).await? {
        for row in db
            .query_all(sql(&format!("SELECT version FROM {HISTORY_TABLE}")))
            .await?
        {
            native.insert(row.try_get::<String>("", "version")?);
        }
        let mut known: HashSet<_> = (0..FORWARDS.len())
            .map(version)
            .chain((0..NATIVE.len()).map(native_version))
            .collect();
        // The baseline's own history row is written by this binary too.
        known.insert(BASELINE_VERSION.to_string());
        if !native.is_subset(&known) {
            return Err(DbErr::Custom(
                "Database has migrations newer than this binary; use the matching release".into(),
            ));
        }
    }
    let missing = |entries: &[(&str, &str)]| {
        entries
            .iter()
            .filter(|(app, name)| !applied.contains(&(app.to_string(), name.to_string())))
            .map(|(app, name)| format!("{app}/{name}"))
            .collect::<Vec<_>>()
    };
    let missing_native = |entries: &[(&str, &str)]| {
        entries
            .iter()
            .enumerate()
            .filter(|(idx, _)| !native.contains(&native_version(*idx)))
            .map(|(_, (app, name))| format!("{app}/{name}"))
            .collect::<Vec<_>>()
    };
    let mut imported_from_django = Vec::new();
    for (index, (app, name)) in FORWARDS.iter().enumerate() {
        if applied.contains(&(app.to_string(), name.to_string()))
            && !native.contains(&version(index))
        {
            imported_from_django.push(format!("{app}/{name}"));
        }
    }
    let mut pending = missing(FORWARDS);
    pending.extend(missing_native(NATIVE));
    Ok(Status {
        baseline_missing: missing(BASELINE),
        pending,
        imported_from_django,
    })
}

/// `check` is read-only: one status probe, no polling, no sleeps. Exits
/// nonzero (via the caller's error rendering) immediately when the schema is
/// not current. A missing history table or an unreachable database is also
/// an error, never a pass.
pub async fn check(db: &impl ConnectionTrait) -> Result<Status, DbErr> {
    let current = status(db).await?;
    current.require_ready()?;
    Ok(current)
}

/// A lock-only transaction spans the entire run, including autocommit DDL.
/// A second connection runs the work; dropping the lock transaction releases
/// ownership even after cancellation. No session advisory locks enter the pool.
pub async fn up(db: &DatabaseConnection) -> Result<Status, DbErr> {
    if db
        .get_postgres_connection_pool()
        .options()
        .get_max_connections()
        < 2
    {
        return Err(DbErr::Custom(
            "Migration runner requires a pool of at least two connections".into(),
        ));
    }
    let lock = db.begin().await?;
    let locked: bool = lock
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT pg_try_advisory_xact_lock($1) AS locked",
            [LOCK_ID.into()],
        ))
        .await?
        .ok_or_else(|| DbErr::Custom("No lock response".into()))?
        .try_get("", "locked")?;
    if !locked {
        return Err(DbErr::Custom(
            "Another Pi Dash migration is running; retry after it finishes".into(),
        ));
    }
    let result = up_locked(db).await;
    lock.rollback().await?;
    result
}

async fn set_timeouts(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    db.execute_unprepared("SET LOCAL lock_timeout = '30s'; SET LOCAL statement_timeout = '10min'")
        .await?;
    Ok(())
}

async fn up_locked(db: &DatabaseConnection) -> Result<Status, DbErr> {
    // Baseline first: `up` boots empty databases (the baseline migration's
    // fresh-install path) and upgrades baseline ones; partially migrated
    // databases are refused so Django can finish them first.
    if let BaselineState::Partial(missing) = baseline_state(db).await? {
        return Err(DbErr::Custom(format!(
            "Partially migrated database; missing: {}. Finish with `manage.py migrate` first, then re-run. No migrations were applied.",
            missing.join(", ")
        )));
    }
    // One transaction per migration; a partial failure leaves the database
    // at the last fully applied migration, and each failure names its
    // migration.
    loop {
        let pending = Migrator::get_pending_migrations(db).await?;
        let Some(next) = pending.first() else {
            break;
        };
        let name = next.name().to_string();
        let tx = db.begin().await?;
        set_timeouts(&tx).await?;
        if let Err(source) = Migrator::up(&tx, Some(1)).await {
            let _ = tx.rollback().await;
            return Err(DbErr::Custom(format!("Migration {name} failed: {source}")));
        }
        tx.commit().await?;
    }
    let result = status(db).await?;
    if !result.ready() {
        return Err(DbErr::Custom(format!(
            "Incomplete migration plan: {:?}",
            result.pending
        )));
    }
    Ok(result)
}

fn version(index: usize) -> String {
    let (app, name) = FORWARDS[index];
    format!("m000001_forward_{index:06}_{app}_{name}")
}

fn native_version(index: usize) -> String {
    let (app, name) = NATIVE[index];
    format!("m000001_native_{index:06}_{app}_{name}")
}

/// Stable history name for the snapshot. Never re-cut under a new name:
/// later schema changes arrive as forward ports, so one name covers every
/// database the migrator ever bootstrapped.
const BASELINE_VERSION: &str = "m380_baseline_db0183_runner0043";

struct Baseline;

impl MigrationName for Baseline {
    fn name(&self) -> &str {
        BASELINE_VERSION
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Baseline {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        match baseline_state(db).await? {
            // Import path: Django already applied everything at the leaf.
            // Record history only; never rerun DDL.
            BaselineState::Current => Ok(()),
            // Fresh-install path: the full snapshot plus history stamps.
            BaselineState::Fresh => {
                db.execute_unprepared(include_str!("../sql/baseline.sql"))
                    .await?;
                Ok(())
            }
            BaselineState::Partial(missing) => Err(DbErr::Custom(format!(
                "Partially migrated database; missing: {}. Finish with `manage.py migrate` first, then re-run.",
                missing.join(", ")
            ))),
        }
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Custom(
            "Forward-only migration: applied migrations are never reversed".into(),
        ))
    }
}

struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        std::iter::once(Box::new(Baseline) as Box<dyn MigrationTrait>)
            .chain((0..FORWARDS.len()).map(|index| {
                Box::new(Port {
                    index,
                    name: version(index),
                }) as Box<dyn MigrationTrait>
            }))
            .chain((0..NATIVE.len()).map(|index| {
                Box::new(PortNative {
                    index,
                    name: native_version(index),
                }) as Box<dyn MigrationTrait>
            }))
            .collect()
    }

    fn migration_table_name() -> sea_orm::sea_query::DynIden {
        use sea_orm::sea_query::IntoIden;
        sea_orm::sea_query::Alias::new(HISTORY_TABLE).into_iden()
    }
}

struct Port {
    index: usize,
    name: String,
}

impl MigrationName for Port {
    fn name(&self) -> &str {
        &self.name
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Port {
    async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Part 2 registers the first forward ports here: each port checks
        // `django_migrations` first and imports (skips DDL for) migrations
        // Django already applied, executes its SQL otherwise, then records
        // the Django row — exactly like the cloud migrator's ports.
        let _ = self.index;
        Err(DbErr::Custom("No forward ports registered yet".into()))
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Custom(
            "Forward-only migration: applied migrations are never reversed".into(),
        ))
    }
}

struct PortNative {
    index: usize,
    name: String,
}

impl MigrationName for PortNative {
    fn name(&self) -> &str {
        &self.name
    }
}

#[async_trait::async_trait]
impl MigrationTrait for PortNative {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Native-only upgrades: additive, idempotent DDL with no Django
        // history row. Existing databases adopt what boot-time `ensure_schema`
        // already created; fresh installs get the tables from the migration.
        // The manager connection works in any context (pooled connection or
        // open transaction): the ensure fns are generic over it.
        match self.index {
            0 => {
                crate::ensure::ensure_rust_tables(manager.get_connection())
                    .await
                    .map_err(|source| DbErr::Custom(format!("rust-owned tables: {source}")))?;
                Ok(())
            }
            _ => Err(DbErr::Custom("No native migrations registered yet".into())),
        }
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Custom(
            "Forward-only migration: applied migrations are never reversed".into(),
        ))
    }
}

/// Error type for the `run_*` entry points (and the CLI): configuration
/// problems (missing/unparsable URLs) versus database failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationError {
    Config(String),
    Db(String),
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationError::Config(message) | MigrationError::Db(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for MigrationError {}

impl From<DbErr> for MigrationError {
    fn from(source: DbErr) -> Self {
        MigrationError::Db(source.to_string())
    }
}

/// The owner/app split (mirrors the cloud migrator): `up` needs the owner
/// URL (`MIGRATOR_DATABASE_URL`) because runtime credentials are never used
/// for DDL; `status`/`check` use the runtime URL and fall back to the owner
/// URL. Empty strings count as missing, like the cloud `nonempty_env`.
pub fn select_up_url(
    migrator_url: Option<&str>,
    runtime_url: Option<&str>,
) -> Result<String, MigrationError> {
    let owner = migrator_url
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            MigrationError::Config(
                "up requires MIGRATOR_DATABASE_URL; runtime credentials are never used for DDL"
                    .into(),
            )
        })?;
    if let Some(runtime) = runtime_url.map(str::trim).filter(|s| !s.is_empty()) {
        if !same_database(runtime, owner) {
            return Err(MigrationError::Config(
                "DATABASE_URL and MIGRATOR_DATABASE_URL must target the same host, port, database and connection options".into(),
            ));
        }
    }
    Ok(owner.to_string())
}

pub fn select_read_url(
    runtime_url: Option<&str>,
    migrator_url: Option<&str>,
) -> Result<String, MigrationError> {
    runtime_url
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| migrator_url.map(str::trim).filter(|s| !s.is_empty()))
        .map(str::to_string)
        .ok_or_else(|| MigrationError::Config("DATABASE_URL is required for status/check".into()))
}

/// Same host, port, database and connection options; credentials may differ.
/// The comparison ignores only the userinfo, like the cloud migrator.
pub fn same_database(a: &str, b: &str) -> bool {
    match (url::Url::parse(a), url::Url::parse(b)) {
        (Ok(a), Ok(b)) => {
            a.host_str() == b.host_str()
                && a.port().unwrap_or(5432) == b.port().unwrap_or(5432)
                && a.path() == b.path()
                && a.query() == b.query()
        }
        _ => false,
    }
}

fn connect_options(url: &str, max_connections: u32) -> ConnectOptions {
    let mut options = ConnectOptions::new(url);
    options
        .max_connections(max_connections)
        .connect_timeout(Duration::from_secs(10))
        .acquire_timeout(Duration::from_secs(10))
        .sqlx_logging(false);
    options
}

/// Connect with at least two connections (the `up` lock transaction plus the
/// worker) and apply pending migrations. Read the URL from
/// `MIGRATOR_DATABASE_URL` via [`select_up_url`].
pub async fn run_up(url: &str) -> Result<Status, MigrationError> {
    let db = Database::connect(connect_options(url, 2)).await?;
    Ok(up(&db).await?)
}

/// Connect with one connection and report the migration state. Read-only.
pub async fn run_status(url: &str) -> Result<Status, MigrationError> {
    let db = Database::connect(connect_options(url, 1)).await?;
    Ok(status(&db).await?)
}

/// Connect with one connection and fail when the schema is not current.
/// Read-only: one probe, no polling, no sleeps.
pub async fn run_check(url: &str) -> Result<Status, MigrationError> {
    let db = Database::connect(connect_options(url, 1)).await?;
    Ok(check(&db).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_table_is_oss_only() {
        // The cloud migrator keeps its own history; the two must never be
        // confused (the cloud binary runs the combined set, OSS only OSS).
        assert_eq!(HISTORY_TABLE, "pidash_schema_migrations");
        assert_ne!(HISTORY_TABLE, "pidash_cloud_schema_migrations");
        assert_ne!(HISTORY_TABLE, "django_migrations");
    }

    #[test]
    fn lock_key_differs_from_cloud() {
        // "PIDASHMO" (OSS migrator) vs "PIDASHMI" (cloud migrator): an OSS
        // `up` and a cloud `up` against one database must not interleave DDL.
        assert_eq!(LOCK_ID, 0x5049_4441_5348_4D4F);
        assert_ne!(LOCK_ID, 0x5049_4441_5348_4D49);
    }

    #[test]
    fn baseline_covers_the_leaf() {
        // The snapshot is cut at db/0183 + runner/0043 (rebased for
        // runner/0044 once NEWPIDASH-372 merges); every app present.
        assert_eq!(BASELINE.len(), 290);
        for leaf in [
            ("db", "0183_merge_ai_completed_and_project_settings"),
            ("runner", "0043_desktop_handoff"),
            ("license", "0006_instance_is_current_version_deprecated"),
            ("prompting", "0007_reseed_directional_relations"),
            ("assistant", "0004_usersttconfig"),
            ("auth", "0012_alter_user_first_name_max_length"),
            ("contenttypes", "0002_remove_content_type_name"),
            ("sessions", "0001_initial"),
            ("django_celery_beat", "0018_improve_crontab_helptext"),
        ] {
            assert!(
                BASELINE.contains(&leaf),
                "baseline must contain {leaf:?} (regenerate if the leaf moved)"
            );
        }
        // `rest_framework.authtoken` ships migrations but is not in
        // INSTALLED_APPS, so Django never records them: they must stay out
        // (verified against a live migrate).
        assert!(
            !BASELINE.iter().any(|(app, _)| *app == "authtoken"),
            "authtoken is not migrated by Django; drop it from the generator"
        );
        let mut seen = HashSet::new();
        for entry in BASELINE {
            assert!(seen.insert(entry), "duplicate baseline entry {entry:?}");
        }
    }

    #[test]
    fn status_ready_iff_nothing_missing() {
        let ready = Status {
            baseline_missing: vec![],
            pending: vec![],
            imported_from_django: vec!["db/0172_example".into()],
        };
        assert!(ready.ready());
        let pending = Status {
            baseline_missing: vec![],
            pending: vec!["runner/0044_example".into()],
            imported_from_django: vec![],
        };
        assert!(!pending.ready());
        let missing = Status {
            baseline_missing: vec!["db/0183_example".into()],
            pending: vec![],
            imported_from_django: vec![],
        };
        assert!(!missing.ready());
    }

    #[test]
    fn ready_gate_names_what_is_missing() {
        let current = Status {
            baseline_missing: vec![],
            pending: vec![],
            imported_from_django: vec![],
        };
        assert!(current.require_ready().is_ok());
        let old = Status {
            baseline_missing: vec!["db/0183_example".into()],
            pending: vec![],
            imported_from_django: vec![],
        };
        let err = old.require_ready().unwrap_err().to_string();
        assert!(err.contains("db/0183_example"), "must name it: {err}");
    }

    #[test]
    fn target_comparison_ignores_only_credentials() {
        assert!(same_database(
            "postgres://app:one@localhost/db",
            "postgres://owner:two@localhost:5432/db"
        ));
        assert!(!same_database(
            "postgres://app@localhost/test",
            "postgres://owner@localhost/prod"
        ));
        assert!(!same_database(
            "postgres://app@localhost/db",
            "postgres://owner@elsewhere/db"
        ));
        assert!(!same_database(
            "postgres://app@localhost/db?options=a",
            "postgres://owner@localhost/db?options=b"
        ));
        // Unparsable URLs never compare equal (fail closed, like the gate).
        assert!(!same_database("not a url", "postgres://owner@localhost/db"));
    }

    #[test]
    fn up_requires_the_owner_url() {
        assert!(select_up_url(None, None).is_err());
        assert!(select_up_url(Some("  "), None).is_err());
        let owner = "postgres://owner:pw@localhost/db";
        assert_eq!(select_up_url(Some(owner), None).unwrap(), owner,);
        assert_eq!(
            select_up_url(Some(owner), Some("postgres://app:pw@localhost/db")).unwrap(),
            owner,
        );
        let err = select_up_url(Some(owner), Some("postgres://app:pw@localhost/other"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("same host, port, database"), "{err}");
    }

    #[test]
    fn read_prefers_the_runtime_url() {
        let runtime = "postgres://app:pw@localhost/db";
        let owner = "postgres://owner:pw@localhost/db";
        assert_eq!(
            select_read_url(Some(runtime), Some(owner)).unwrap(),
            runtime
        );
        assert_eq!(select_read_url(None, Some(owner)).unwrap(), owner);
        assert_eq!(select_read_url(Some(" "), Some(owner)).unwrap(), owner);
        assert!(select_read_url(None, None).is_err());
        assert!(select_read_url(Some(" "), Some(" ")).is_err());
    }

    #[test]
    fn migrator_registers_no_history_rewrites() {
        // SeaORM history rows are versioned `m…` names under our own table;
        // the Django rows are written by the ports, never by the framework.
        // (`down` is never implemented: applied migrations are never
        // reversed. Live-DB tests call it and assert the error.)
        let table = format!("{:?}", Migrator::migration_table_name());
        assert!(table.contains(HISTORY_TABLE), "unexpected table: {table}");
        let names = Migrator::migrations()
            .iter()
            .map(|m| m.name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(names, [BASELINE_VERSION.to_string(), native_version(0)]);
        // Forwards land next: runner/0044 once NEWPIDASH-372 merges.
        assert!(FORWARDS.is_empty());
        assert_eq!(NATIVE, &[("rust", "0002_ensure_schema_tables")]);
    }

    #[test]
    fn baseline_version_is_stable() {
        // The snapshot is cut once; later changes arrive as forwards, so
        // every bootstrapped database shares one history name forever.
        assert_eq!(BASELINE_VERSION, "m380_baseline_db0183_runner0043");
    }
}
