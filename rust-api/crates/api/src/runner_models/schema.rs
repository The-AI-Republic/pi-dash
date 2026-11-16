#![forbid(unsafe_code)]

//! Schema + seed for the runner LLM model catalog (NEWPIDASH-169).
//!
//! The table is Rust-owned, like `rust_job_queue`: created at serve/worker
//! boot, never through a Django migration. DDL is the allowed raw-SQL
//! exception (sea-query cannot express `CREATE TABLE IF NOT EXISTS`);
//! every seed row goes through the SeaORM entity.

use chrono::Utc;
use sea_orm::{EntityTrait, PaginatorTrait, Set};
use sqlx::PgPool;

use pidash_db::entity::rust_runner_model_catalog;
use pidash_db::sea_orm_connection;

/// `(agent, model, label, reasoning_efforts, sort_order, is_preselected)`.
type SeedRow = (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
    i32,
    bool,
);

/// Mirrors `apps/web_new/.../addRunner/catalog.ts` on day one: 6 Claude
/// rows (Opus 4.8 preselected), 7 Codex rows (each offering the four
/// efforts; the picker crosses them), 12 Cursor rows, 2 Grok rows —
/// 27 rows, day-one behaviour unchanged.
const SEED: &[SeedRow] = &[
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
];

/// Seed row count on a fresh table (6 + 7 + 12 + 2).
pub const SEED_COUNT: usize = 27;

/// DDL for the catalog table. Idempotent: safe to run on every boot.
pub fn ensure_schema_sql() -> &'static str {
    "CREATE TABLE IF NOT EXISTS rust_runner_model_catalog (\
        id UUID PRIMARY KEY, \
        agent TEXT NOT NULL CHECK (agent IN ('claude-code','codex','cursor-agent','open-claw','grok','muse-code')), \
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

/// DDL for the picker order index. Separate statement: Postgres rejects
/// multiple commands in one prepared statement.
pub fn ensure_index_sql() -> &'static str {
    "CREATE INDEX IF NOT EXISTS rust_runner_model_catalog_agent_sort_idx \
        ON rust_runner_model_catalog (agent, sort_order)"
}

/// Create the table + index, then seed the 27-row catalog when the table
/// is empty (delete-then-reboot resurrects only when every row is gone;
/// a single delete never resurrects, and admin edits are never
/// overwritten).
pub async fn ensure_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    // raw-sql: Rust-owned DDL (CREATE TABLE/INDEX IF NOT EXISTS) is an allowed exception per orm-migration.md §6: sea-query cannot express DDL, and the catalog table has no Django counterpart.
    sqlx::query(ensure_schema_sql()).execute(pool).await?;
    // raw-sql: Rust-owned DDL (CREATE TABLE/INDEX IF NOT EXISTS) is an allowed exception per orm-migration.md §6: sea-query cannot express DDL, and the catalog table has no Django counterpart.
    sqlx::query(ensure_index_sql()).execute(pool).await?;
    let db = sea_orm_connection(pool);
    let count = rust_runner_model_catalog::Entity::find()
        .count(&db)
        .await
        .map_err(|_| sqlx::Error::Decode(Box::new(std::io::Error::other("seed count failed"))))?;
    if count > 0 {
        return Ok(());
    }
    let now = Utc::now().fixed_offset();
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
            .exec(&db)
            .await
            .map_err(|_| {
                sqlx::Error::Decode(Box::new(std::io::Error::other("seed insert failed")))
            })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_has_27_rows_with_claude_opus_preselected() {
        assert_eq!(SEED.len(), SEED_COUNT);
        let preselected: Vec<_> = SEED.iter().filter(|row| row.5).collect();
        assert_eq!(preselected.len(), 1);
        assert_eq!(
            (preselected[0].0, preselected[0].1),
            ("claude-code", "claude-opus-4-8")
        );
    }

    #[test]
    fn seed_covers_codex_cursor_and_grok() {
        let count = |agent: &str| SEED.iter().filter(|row| row.0 == agent).count();
        assert_eq!(count("claude-code"), 6);
        assert_eq!(count("codex"), 7);
        assert_eq!(count("cursor-agent"), 12);
        assert_eq!(count("grok"), 2);
        assert_eq!(count("open-claw"), 0);
        assert_eq!(count("muse-code"), 0);
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
        for agent in ["claude-code", "codex", "cursor-agent", "grok"] {
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
