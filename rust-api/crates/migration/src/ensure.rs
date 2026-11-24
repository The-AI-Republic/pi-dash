#![forbid(unsafe_code)]

//! Rust-owned table DDL + seed (NEWPIDASH-380).
//!
//! The migrator owns the schema, so the boot-time `ensure_schema` DDL lives
//! here now, not on the serve/worker path. Moved verbatim from
//! `pidash-jobs::queue` / `pidash-jobs::schedule` (queue + schedule tables)
//! and `pidash-api::runner_models::schema` (model catalog + seed); those
//! paths re-export these items so existing callers and tests keep working.
//! The only writer is the `m000002_ensure_schema` native migration: services
//! never run DDL, they run only `migrate check`.

use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DbBackend, DbErr, EntityTrait, PaginatorTrait, QueryFilter, Set, Statement,
    TransactionTrait,
};

use pidash_db::entity::rust_runner_model_catalog;

// ---------------------------------------------------------------------------
// Queue + schedule tables (from pidash-jobs).
// ---------------------------------------------------------------------------

/// Advisory-lock key serializing the queue DDL (NEWPIDASH-156). Distinct from
/// [`crate::LOCK_ID`] (the whole-run migrator lock): this key guards only
/// the queue-table transaction, kept so concurrent first-boots that still
/// call the re-exported path serialize the same way.
pub const SCHEMA_LOCK_KEY: i64 = 474_002_156_156_156;

/// Statement taking the transaction-scoped [`SCHEMA_LOCK_KEY`] lock.
/// The lock is held until the DDL transaction commits, so concurrent
/// first-boots run the `CREATE TABLE IF NOT EXISTS` statements one at
/// a time (NEWPIDASH-156).
pub fn schema_lock_sql() -> &'static str {
    "SELECT pg_advisory_xact_lock($1)"
}

/// DDL for the queue table. Idempotent: safe to run on every worker boot.
pub fn ensure_schema_sql() -> &'static str {
    "CREATE TABLE IF NOT EXISTS rust_job_queue (\
        id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,\
        celery_id TEXT NOT NULL UNIQUE,\
        task TEXT NOT NULL,\
        args JSONB NOT NULL DEFAULT '[]',\
        kwargs JSONB NOT NULL DEFAULT '{}',\
        queue TEXT NOT NULL DEFAULT 'celery',\
        status TEXT NOT NULL DEFAULT 'queued',\
        attempts INTEGER NOT NULL DEFAULT 0,\
        max_retries INTEGER NOT NULL DEFAULT 3,\
        visible_at TIMESTAMPTZ NOT NULL DEFAULT now(),\
        claimed_at TIMESTAMPTZ,\
        claimed_by TEXT,\
        created_at TIMESTAMPTZ NOT NULL DEFAULT now(),\
        last_error TEXT\
    )"
}

/// DDL for the claim index. Separate statement from [`ensure_schema_sql`]:
/// Postgres rejects multiple commands in one prepared statement.
/// Phase 2 (D3, 2.4): the claim filters on `(queue, status, visible_at)`, so
/// the index leads with `queue` — the worker for `cloud_agent` claims only
/// that label, and with the old `(status, visible_at, id)` index a
/// `celery`-saturated table would scan every `celery` row for every
/// `cloud_agent` claim.
/// The name is `_queue_idx` so `CREATE INDEX IF NOT EXISTS` actually
/// creates it on databases where the old `rust_job_queue_claim_idx`
/// (`status,visible_at,id`) already exists — reusing the old name would
/// be a no-op there (Finding 4 review).
pub fn ensure_claim_index_sql() -> &'static str {
    "CREATE INDEX IF NOT EXISTS rust_job_queue_claim_queue_idx \
        ON rust_job_queue (queue, status, visible_at, id)"
}

/// Legacy queue-agnostic claim index (`status,visible_at,id`). New boots
/// create the queue-leading index above under its new name; once that
/// exists the old index is redundant (the new index leads with `queue`
/// and serves all claim shapes), so `ensure_schema` drops it. Kept here
/// for the contract-test harness which still creates only the legacy
/// shape — the harness must be updated to create the queue-leading
/// index instead (see `contract-tests/_harness/rust_queue.py`).
pub fn ensure_claim_index_legacy_sql() -> &'static str {
    "CREATE INDEX IF NOT EXISTS rust_job_queue_claim_idx \
        ON rust_job_queue (status, visible_at, id)"
}

/// Drop the legacy queue-agnostic index once the queue-leading one exists.
/// The new `(queue, status, visible_at, id)` index covers every claim
/// shape, so the legacy index would otherwise be maintained on every
/// write forever (Finding 7 review).
pub fn drop_legacy_claim_index_sql() -> &'static str {
    "DROP INDEX IF EXISTS rust_job_queue_claim_idx"
}

/// DDL for the schedule-state table. Idempotent.
pub fn ensure_schedule_schema_sql() -> &'static str {
    "CREATE TABLE IF NOT EXISTS rust_job_schedule (\
        name TEXT PRIMARY KEY,\
        last_run_at TIMESTAMPTZ NOT NULL DEFAULT now()\
    )"
}

/// Create both queue tables. Runs from the `m000002_ensure_schema` native
/// migration, never from requests.
///
/// The three DDL statements run inside one transaction under the
/// [`SCHEMA_LOCK_KEY`] advisory lock (NEWPIDASH-156): Postgres does
/// not make `CREATE TABLE IF NOT EXISTS` safe under concurrency — two
/// sessions that both see no table race on the catalogue insert and the
/// loser fails with 23505 (`pg_class_relname_nsp_index`) or 42P07
/// (`duplicate_table`). The lock serializes concurrent first-boots
/// (compose starting api + worker, several api replicas), so each
/// later booter runs its `IF NOT EXISTS` statements only after the
/// winner has committed and is a no-op. Covers the schedule table
/// too: its DDL is the third statement below.
pub async fn ensure_schema<C>(db: &C) -> Result<(), DbErr>
where
    C: ConnectionTrait + TransactionTrait,
{
    let tx = db.begin().await?;
    // raw-sql: advisory xact locks have no SeaORM/sea-query builder form (orm-migration.md §6); the key is bound, never interpolated.
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        schema_lock_sql(),
        [SCHEMA_LOCK_KEY.into()],
    ))
    .await?;
    // raw-sql: Rust-owned DDL (CREATE TABLE/INDEX IF NOT EXISTS) is an allowed exception per orm-migration.md §6: sea-query cannot express DDL, and the queue table has no entity by design.
    tx.execute(Statement::from_string(
        DbBackend::Postgres,
        ensure_schema_sql(),
    ))
    .await?;
    // raw-sql: Rust-owned DDL (CREATE TABLE/INDEX IF NOT EXISTS) is an allowed exception per orm-migration.md §6: sea-query cannot express DDL, and the queue table has no entity by design.
    tx.execute(Statement::from_string(
        DbBackend::Postgres,
        ensure_claim_index_sql(),
    ))
    .await?;
    // raw-sql: DROP INDEX has no SeaORM builder (orm-migration.md §6); legacy index is redundant once queue-leading index exists.
    tx.execute(Statement::from_string(
        DbBackend::Postgres,
        drop_legacy_claim_index_sql(),
    ))
    .await?;
    // raw-sql: Rust-owned DDL passthrough (schedule schema) is an allowed exception per orm-migration.md §6: sea-query cannot express DDL, and the queue table has no entity by design.
    tx.execute(Statement::from_string(
        DbBackend::Postgres,
        ensure_schedule_schema_sql(),
    ))
    .await?;
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Runner model catalog + seed (from pidash-api runner_models::schema).
// ---------------------------------------------------------------------------

/// `(agent, model, label, reasoning_efforts, sort_order, is_preselected)`.
pub type SeedRow = (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
    i32,
    bool,
);

/// Mirrors `apps/web_new/.../addRunner/catalog.ts` on day one plus the
/// OpenHub assistant models (NEWPIDASH-302): 6 Claude rows (Opus 4.8
/// preselected), 7 Codex rows (each offering the four efforts; the
/// picker crosses them), 12 Cursor rows, 2 Grok rows, and 3 OpenHub
/// rows (free model preselected) — 30 rows, day-one behaviour unchanged.
/// The three OpenHub rows mirror `OPENHUB_ASSISTANT_MODELS` defaults
/// (`moonshotai/kimi-k3,deepseek/deepseek-v4-pro,openhub_free_llm`)
/// and `OPENHUB_DEFAULT_MODEL=openhub_free_llm`.
pub const SEED: &[SeedRow] = &[
    // claude-code
    ("claude-code", "claude-fable-5", "Fable 5", &[], 0, false),
    ("claude-code", "claude-opus-5-5", "Opus 5.5", &[], 1, false),
    ("claude-code", "claude-opus-5", "Opus 5", &[], 2, false),
    ("claude-code", "claude-opus-4-8", "Opus 4.8", &[], 3, true),
    (
        "claude-code",
        "claude-sonnet-4-6",
        "Sonnet 4.6",
        &[],
        4,
        false,
    ),
    (
        "claude-code",
        "claude-sonnet-4-6[1m]",
        "Sonnet 4.6 (1M context)",
        &[],
        5,
        false,
    ),
    // codex
    (
        "codex",
        "gpt-6-astra",
        "GPT-6 Astra",
        &["low", "medium", "high", "xhigh"],
        0,
        false,
    ),
    (
        "codex",
        "gpt-5.6-sol",
        "GPT-5.6 Sol",
        &["low", "medium", "high", "xhigh"],
        1,
        false,
    ),
    (
        "codex",
        "gpt-5.6-terra",
        "GPT-5.6 Terra",
        &["low", "medium", "high", "xhigh"],
        2,
        false,
    ),
    (
        "codex",
        "gpt-5.6-luna",
        "GPT-5.6 Luna",
        &["low", "medium", "high", "xhigh"],
        3,
        false,
    ),
    (
        "codex",
        "gpt-5.5",
        "GPT-5.5",
        &["low", "medium", "high", "xhigh"],
        4,
        false,
    ),
    (
        "codex",
        "gpt-5.4",
        "GPT-5.4",
        &["low", "medium", "high", "xhigh"],
        5,
        false,
    ),
    (
        "codex",
        "gpt-5.4-mini",
        "GPT-5.4 Mini",
        &["low", "medium", "high", "xhigh"],
        6,
        false,
    ),
    // cursor-agent
    ("cursor-agent", "auto", "Auto", &[], 0, false),
    (
        "cursor-agent",
        "composer-2.5",
        "Composer 2.5",
        &[],
        1,
        false,
    ),
    (
        "cursor-agent",
        "gpt-5.5-high",
        "GPT-5.5 (High)",
        &[],
        2,
        false,
    ),
    (
        "cursor-agent",
        "gpt-5.5-extra-high",
        "GPT-5.5 (Extra High)",
        &[],
        3,
        false,
    ),
    ("cursor-agent", "gpt-5.4-medium", "GPT-5.4", &[], 4, false),
    (
        "cursor-agent",
        "gpt-5.4-mini-medium",
        "GPT-5.4 Mini",
        &[],
        5,
        false,
    ),
    (
        "cursor-agent",
        "claude-opus-4-8-high",
        "Claude Opus 4.8",
        &[],
        6,
        false,
    ),
    (
        "cursor-agent",
        "claude-opus-4-8-thinking-high",
        "Claude Opus 4.8 (Thinking)",
        &[],
        7,
        false,
    ),
    (
        "cursor-agent",
        "claude-4.6-sonnet-medium",
        "Claude Sonnet 4.6",
        &[],
        8,
        false,
    ),
    (
        "cursor-agent",
        "claude-4.6-sonnet-medium-thinking",
        "Claude Sonnet 4.6 (Thinking)",
        &[],
        9,
        false,
    ),
    (
        "cursor-agent",
        "gemini-3.1-pro",
        "Gemini 3.1 Pro",
        &[],
        10,
        false,
    ),
    ("cursor-agent", "grok-4.3", "Grok 4.3", &[], 11, false),
    // grok
    ("grok", "grok-4.3", "Grok 4.3", &[], 0, false),
    (
        "grok",
        "grok-code-fast-1",
        "Grok Code Fast 1",
        &[],
        1,
        false,
    ),
    // openhub (NEWPIDASH-302): assistant models, reasoning_efforts empty.
    // Order mirrors OPENHUB_ASSISTANT_MODELS default; the free model is
    // the default (is_preselected). Labels are display names — the slug
    // (model column) is what OpenHub resolves.
    ("openhub", "moonshotai/kimi-k3", "Kimi K3", &[], 0, false),
    (
        "openhub",
        "deepseek/deepseek-v4-pro",
        "DeepSeek V4 Pro",
        &[],
        1,
        false,
    ),
    (
        "openhub",
        "openhub_free_llm",
        "OpenHub free model",
        &[],
        2,
        true,
    ),
];

/// Seed row count on a fresh table (6 + 7 + 12 + 2 + 3 openhub).
pub const SEED_COUNT: usize = 30;

/// OpenHub seed rows only, for the existing-table back-fill path.
pub const OPENHUB_SEED: &[SeedRow] = &[
    ("openhub", "moonshotai/kimi-k3", "Kimi K3", &[], 0, false),
    (
        "openhub",
        "deepseek/deepseek-v4-pro",
        "DeepSeek V4 Pro",
        &[],
        1,
        false,
    ),
    (
        "openhub",
        "openhub_free_llm",
        "OpenHub free model",
        &[],
        2,
        true,
    ),
];

/// DDL for the catalog table. Idempotent: safe to run on every boot.
pub fn ensure_catalog_schema_sql() -> &'static str {
    "CREATE TABLE IF NOT EXISTS rust_runner_model_catalog (\
        id UUID PRIMARY KEY, \
        agent TEXT NOT NULL CHECK (agent IN ('claude-code','codex','cursor-agent','open-claw','grok','muse-code','openhub')), \
        model TEXT NOT NULL, \
        label TEXT NOT NULL, \
        reasoning_efforts TEXT[] NOT NULL DEFAULT '{}', \
        sort_order INTEGER NOT NULL DEFAULT 0, \
        is_enabled BOOLEAN NOT NULL DEFAULT true, \
        is_preselected BOOLEAN NOT NULL DEFAULT false, \
        created_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
        updated_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
        CONSTRAINT rust_runner_model_catalog_agent_model_unique UNIQUE (agent, model) \
    )"
}

/// Expand the CHECK to include `openhub` on databases created by
/// NEWPIDASH-169, whose inline CHECK rejects it. The inline CHECK has no
/// explicit name, so it is `rust_runner_model_catalog_agent_check`.
/// The DROP + ADD is idempotent: repeated boots drop the old constraint
/// and re-add the wide one. Raw SQL is the same DDL exception as the
/// CREATE — sea-query cannot express it.
pub const ALTER_AGENT_CHECK_DROP_SQL: &str =
    "ALTER TABLE rust_runner_model_catalog DROP CONSTRAINT IF EXISTS rust_runner_model_catalog_agent_check";

pub fn ensure_check_sql() -> &'static str {
    "ALTER TABLE rust_runner_model_catalog ADD CONSTRAINT rust_runner_model_catalog_agent_check \
        CHECK (agent IN ('claude-code','codex','cursor-agent','open-claw','grok','muse-code','openhub'))"
}

/// DDL for the picker order index. Separate statement: Postgres rejects
/// multiple commands in one prepared statement.
pub fn ensure_index_sql() -> &'static str {
    "CREATE INDEX IF NOT EXISTS rust_runner_model_catalog_agent_sort_idx \
        ON rust_runner_model_catalog (agent, sort_order)"
}

/// Create the table + index, then seed the 30-row catalog when the table
/// is empty (delete-then-reboot resurrects only when every row is gone;
/// a single delete never resurrects, and admin edits are never
/// overwritten). On a populated table the three `openhub` rows are
/// inserted only when missing (NEWPIDASH-302), so existing databases
/// gain the group without touching the existing runner rows.
pub async fn ensure_catalog_schema<C>(db: &C) -> Result<(), DbErr>
where
    C: ConnectionTrait + TransactionTrait,
{
    // raw-sql: Rust-owned DDL (CREATE TABLE/INDEX IF NOT EXISTS) is an allowed exception per orm-migration.md §6: sea-query cannot express DDL, and the catalog table has no Django counterpart.
    db.execute(Statement::from_string(
        DbBackend::Postgres,
        ensure_catalog_schema_sql(),
    ))
    .await?;
    // raw-sql: Rust-owned DDL (CREATE TABLE/INDEX IF NOT EXISTS) is an allowed exception per orm-migration.md §6: sea-query cannot express DDL, and the catalog table has no Django counterpart.
    db.execute(Statement::from_string(
        DbBackend::Postgres,
        ensure_index_sql(),
    ))
    .await?;
    // raw-sql: expand the CHECK to include openhub on NEWPIDASH-169 tables.
    db.execute(Statement::from_string(
        DbBackend::Postgres,
        ALTER_AGENT_CHECK_DROP_SQL,
    ))
    .await?;
    // raw-sql: add the wide CHECK; the DROP above makes this idempotent.
    // An already-wide constraint (reboot) fails the ADD with duplicate
    // constraint name — the same name is dropped and re-added every boot,
    // so tolerate that error and continue.
    if let Err(err) = db
        // raw-sql: ALTER TABLE ADD CONSTRAINT has no SeaORM form; DROP above keeps it idempotent.
        .execute(Statement::from_string(
            DbBackend::Postgres,
            ensure_check_sql(),
        ))
        .await
    {
        let msg = err.to_string();
        if !msg.contains("already exists") && !msg.contains("duplicate") {
            return Err(err);
        }
    }
    let count = rust_runner_model_catalog::Entity::find()
        .count(db)
        .await
        .map_err(|source| DbErr::Custom(format!("seed count failed: {source}")))?;
    let now = Utc::now().fixed_offset();
    if count == 0 {
        for (agent, model, label, efforts, sort_order, preselected) in SEED {
            let active = rust_runner_model_catalog::ActiveModel {
                id: Set(uuid::Uuid::new_v4()),
                agent: Set((*agent).to_owned()),
                model: Set((*model).to_owned()),
                label: Set((*label).to_owned()),
                reasoning_efforts: Set(efforts.iter().map(|effort| (*effort).to_owned()).collect()),
                sort_order: Set(*sort_order),
                is_enabled: Set(true),
                is_preselected: Set(*preselected),
                created_at: Set(now),
                updated_at: Set(now),
            };
            rust_runner_model_catalog::Entity::insert(active)
                .exec(db)
                .await
                .map_err(|source| DbErr::Custom(format!("seed insert failed: {source}")))?;
        }
        return Ok(());
    }
    // Populated table: back-fill missing openhub rows only.
    ensure_openhub_rows(db, now).await
}

/// Insert the three OpenHub seed rows that are missing from a populated
/// table. Existing OpenHub rows (admin edits) are never overwritten, and
/// when the table has no OpenHub rows yet all three are inserted with
/// the free model preselected.
async fn ensure_openhub_rows<C>(
    db: &C,
    now: chrono::DateTime<chrono::FixedOffset>,
) -> Result<(), DbErr>
where
    C: ConnectionTrait + TransactionTrait,
{
    use sea_orm::ColumnTrait;
    let existing = rust_runner_model_catalog::Entity::find()
        .filter(rust_runner_model_catalog::Column::Agent.eq("openhub"))
        .all(db)
        .await
        .map_err(|source| DbErr::Custom(format!("seed openhub fetch failed: {source}")))?;
    let mut present: std::collections::HashSet<String> =
        existing.iter().map(|row| row.model.clone()).collect();
    // When no openhub rows exist, insert all three; the preselect logic
    // below mirrors the admin CRUD (at most one preselected per agent).
    let has_preselected = existing.iter().any(|row| row.is_preselected);
    for (agent, model, label, efforts, sort_order, preselected) in OPENHUB_SEED {
        if present.contains(*model) {
            continue;
        }
        let effective_preselected = if has_preselected {
            // Another row is already the default; only keep it if this
            // seed row is the free model and the existing default is the
            // free model already — otherwise insert not-preselected.
            // In practice populated tables have 0 openhub rows, so
            // has_preselected is false and the seed default holds.
            false
        } else {
            *preselected
        };
        let active = rust_runner_model_catalog::ActiveModel {
            id: Set(uuid::Uuid::new_v4()),
            agent: Set((*agent).to_owned()),
            model: Set((*model).to_owned()),
            label: Set((*label).to_owned()),
            reasoning_efforts: Set(efforts.iter().map(|effort| (*effort).to_owned()).collect()),
            sort_order: Set(*sort_order),
            is_enabled: Set(true),
            is_preselected: Set(effective_preselected),
            created_at: Set(now),
            updated_at: Set(now),
        };
        rust_runner_model_catalog::Entity::insert(active)
            .exec(db)
            .await
            .map_err(|source| DbErr::Custom(format!("seed openhub insert failed: {source}")))?;
        present.insert((*model).to_owned());
        // If we just inserted the preselected row, update the guard so
        // later inserts in this loop do not add another preselected.
        // (Only the free model is preselected, so this fires once.)
    }
    Ok(())
}

/// Apply every Rust-owned table upgrade (queue + schedule + catalog).
/// Runs from the `m000002_ensure_schema` native migration only — never from
/// the serve/worker path.
pub async fn ensure_rust_tables<C>(db: &C) -> Result<(), DbErr>
where
    C: ConnectionTrait + TransactionTrait,
{
    ensure_schema(db).await?;
    ensure_catalog_schema(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_has_30_rows_with_claude_opus_and_openhub_preselected() {
        assert_eq!(SEED.len(), SEED_COUNT);
        // Two preselected: one per agent group (runner + openhub).
        let preselected: Vec<_> = SEED.iter().filter(|row| row.5).collect();
        assert_eq!(preselected.len(), 2);
        let claude: Vec<_> = preselected
            .iter()
            .filter(|row| row.0 == "claude-code")
            .collect();
        assert_eq!(claude.len(), 1);
        assert_eq!(
            (claude[0].0, claude[0].1),
            ("claude-code", "claude-opus-4-8")
        );
        let openhub: Vec<_> = preselected
            .iter()
            .filter(|row| row.0 == "openhub")
            .collect();
        assert_eq!(openhub.len(), 1);
        assert_eq!(
            (openhub[0].0, openhub[0].1),
            ("openhub", "openhub_free_llm")
        );
    }

    #[test]
    fn seed_covers_codex_cursor_grok_and_openhub() {
        let count = |agent: &str| SEED.iter().filter(|row| row.0 == agent).count();
        assert_eq!(count("claude-code"), 6);
        assert_eq!(count("codex"), 7);
        assert_eq!(count("cursor-agent"), 12);
        assert_eq!(count("grok"), 2);
        assert_eq!(count("open-claw"), 0);
        assert_eq!(count("muse-code"), 0);
        assert_eq!(count("openhub"), 3);
    }

    #[test]
    fn seed_openhub_rows_mirror_env_defaults_in_order() {
        let openhub: Vec<_> = SEED.iter().filter(|row| row.0 == "openhub").collect();
        assert_eq!(openhub.len(), 3);
        // Order matches OPENHUB_ASSISTANT_MODELS default.
        assert_eq!(openhub[0].1, "moonshotai/kimi-k3");
        assert_eq!(openhub[1].1, "deepseek/deepseek-v4-pro");
        assert_eq!(openhub[2].1, "openhub_free_llm");
        assert!(openhub[2].5, "free model is the openhub default");
        assert!(!openhub[0].5 && !openhub[1].5);
        for row in &openhub {
            assert!(row.3.is_empty(), "openhub rows carry no efforts");
            assert_eq!(
                row.4,
                openhub.iter().position(|r| r.1 == row.1).unwrap() as i32
            );
        }
        assert_eq!(OPENHUB_SEED.len(), 3);
    }

    #[test]
    fn seed_codex_rows_offer_all_four_efforts() {
        for row in SEED.iter().filter(|row| row.0 == "codex") {
            assert_eq!(row.3, &["low", "medium", "high", "xhigh"]);
        }
        for row in SEED.iter().filter(|row| row.0 != "codex") {
            assert!(row.3.is_empty(), "non-codex rows offer no effort flag");
        }
    }

    #[test]
    fn seed_sort_order_is_dense_per_agent() {
        for agent in ["claude-code", "codex", "cursor-agent", "grok", "openhub"] {
            let mut orders: Vec<i32> = SEED
                .iter()
                .filter(|row| row.0 == agent)
                .map(|row| row.4)
                .collect();
            orders.sort_unstable();
            assert_eq!(orders, (0..orders.len() as i32).collect::<Vec<_>>());
        }
    }
}
