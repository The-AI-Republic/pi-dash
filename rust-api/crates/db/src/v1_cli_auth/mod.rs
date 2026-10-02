//! D-22 CLI auth + runner v1 db surface (db layer, stage 5).
//!
//! Ports the db slice PIDASHCONV-533 owns — the one unit this domain still
//! needs after D-17 merged the auth half (PIDASHCONV-342 #808,
//! PIDASHCONV-343 #817; `auth_oauth::routes()` already serves all six
//! `api/v1/auth/*` paths):
//!
//! * `Runner` (`apps/api/pi_dash/runner/models.py:382-687`,
//!   `db_table = "runner"`, plain `models.Model` — no `deleted_at`, no
//!   soft-delete manager) and `Visibility`
//!   (`runner/models.py:186-187`, `PRIVATE = 0`).
//! * The by-pk lookup (`api/views/runner.py:44-45`,
//!   `Runner.objects.filter(pk=runner_id).first()`) and the guard-facts
//!   read (`:48-51`, `visibility` + `owner_id` off the fetched row).
//!
//! * [`models`] — the `runner` table (V1CLIAUTH-F2): `TABLE`, `ORDERING`,
//!   and `COLUMNS` in Django field order, the row struct, FK / unique /
//!   default consts, and the status / visibility / provisioning choice
//!   consts. `#[cfg(test)]` asserts the columns field-for-field against
//!   `rust-api/fixtures/v1_cli_auth/`
//!   `V1CLIAUTH-F2.runner.columns.json` (PIDASHCONV-526).
//! * [`queries`] — the by-pk lookup plus the guard-facts projection
//!   (V1CLIAUTH-F3): same SQL semantics (plain `WHERE id = $1`, no
//!   deleted filter, no join). `#[cfg(test)]` asserts the SQL text and
//!   the row shape against `V1CLIAUTH-F3.lookup.sql` and
//!   `V1CLIAUTH-F3.lookup.rows.json`.
//!
//! The guard kernel (`can_view_runner` / `can_manage_runner`) already
//! lives read-only in `pidash_auth::permissions::runner` and is NOT
//! reimplemented here; the handler layer (PIDASHCONV-538) feeds it the
//! [`queries::runner_lookup::GuardFacts`] projected off the Q1 row.
//! D-13's future `Runner` write model coexists in a different module over
//! the same table.
//!
//! Wiring note: the crate root declares `pub mod v1_cli_auth;` (one-line
//! wiring in the port PR, following the PIDASHCONV-352 precedent).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod models;
pub mod queries;

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    pub(crate) fn fixtures_dir() -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/v1_cli_auth")
    }

    pub(crate) fn fixture_file(name: &str) -> serde_json::Value {
        let body = std::fs::read_to_string(fixtures_dir().join(name))
            .unwrap_or_else(|e| panic!("read {name}: {e}"));
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("{name} is valid JSON: {e}"))
    }

    pub(crate) fn fixture_text(name: &str) -> String {
        std::fs::read_to_string(fixtures_dir().join(name))
            .unwrap_or_else(|e| panic!("read {name}: {e}"))
    }

    /// Column names from an F2-style fixture (`columns[].name`). The F2
    /// names are already Django attnames (`owner_id`, `workspace_id`,
    /// `dev_machine_id`, `pod_id`), so no `_id` transform is needed.
    pub(crate) fn fixture_columns(doc: &serde_json::Value) -> Vec<String> {
        doc["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("fixture has columns array"))
            .iter()
            .map(|c| c["name"].as_str().expect("column has name").to_string())
            .collect()
    }

    /// Copy a column const into an owned vec so assertions compare two
    /// runtime values.
    pub(crate) fn owned_columns(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }
}
