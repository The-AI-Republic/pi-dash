#![forbid(unsafe_code)]

//! Runner LLM model catalog (NEWPIDASH-169, NEWPIDASH-302): table shape,
//! seed and guards.
//!
//! The table is Rust-owned, like `rust_job_queue`: created by the
//! `m000002_ensure_schema` native migration (NEWPIDASH-380), never through
//! a Django migration and never at serve/worker boot. DDL is the allowed
//! raw-SQL exception (sea-query cannot express `CREATE TABLE IF NOT
//! EXISTS`); every seed row goes through the SeaORM entity.

/// Catalog DDL + seed now live in the migrator (NEWPIDASH-380):
/// `pidash-migration::ensure` owns them, the `m000002_ensure_schema` native
/// migration runs them, and services run only `migrate check`. Re-exported
/// here so existing callers and tests keep compiling.
pub use pidash_migration::ensure::{
    ensure_catalog_schema as ensure_schema, ensure_catalog_schema_sql as ensure_schema_sql,
    ensure_check_sql, ensure_index_sql, SeedRow, ALTER_AGENT_CHECK_DROP_SQL, OPENHUB_SEED, SEED,
    SEED_COUNT,
};

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
