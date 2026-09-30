//! D-17 authentication table models (db layer, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/social_connection.py:1-43`
//! (`SocialLoginConnection`) and the device-flow slice of
//! `apps/api/pi_dash/db/models/api.py:24-32` (the `generate_device_code` /
//! `generate_user_code` generators) + `:63-94` (`CLIDeviceCode`), adopting
//! the Django-owned schema column-for-column. Migrations are not ported;
//! Django stays schema owner until switchover.
//!
//! * [`models`] — the two tables (PIDASHCONV-326): `TABLE`, `ORDERING`, and
//!   `COLUMNS` consts in Django `_meta` field order with attnames
//!   (`user` -> `user_id`, `workspace` -> `workspace_id`,
//!   `created_by` -> `created_by_id`), row structs, FK / unique / default
//!   consts, and the two generator functions. `#[cfg(test)]` asserts the
//!   columns field-for-field against `rust-api/fixtures/auth_oauth/`
//!   `F1_social_login_connection.columns.json` (PIDASHCONV-324,
//!   AUTHOAUTH-F1) and `F2_cli_device_code.columns.json` (AUTHOAUTH-F2).
//! * [`queries`] — account upsert, `deactivate_api_token`, device-machine
//!   touch / get-or-create / rotate (PIDASHCONV-327, AUTHOAUTH-F6/F7/F8).
//!
//! # Wiring note
//!
//! The crate root declares `pub mod auth_oauth;` (one-line wiring in the
//! port PR, following the PIDASHCONV-284 precedent).

pub mod models;
pub mod queries;

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    pub(crate) fn fixtures_dir() -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/auth_oauth")
    }

    pub(crate) fn fixture_file(name: &str) -> serde_json::Value {
        let body = std::fs::read_to_string(fixtures_dir().join(name))
            .unwrap_or_else(|e| panic!("read {name}: {e}"));
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("{name} is valid JSON: {e}"))
    }

    /// Column names from an F1/F2-style fixture (`columns[].name`), with
    /// Django attnames: `ForeignKey` entries gain the `_id` suffix.
    pub(crate) fn fixture_columns(doc: &serde_json::Value, key: &str) -> Vec<String> {
        let section = if key.is_empty() {
            doc.clone()
        } else {
            doc[key].clone()
        };
        section["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("fixture section {key} has columns array"))
            .iter()
            .map(|c| {
                let name = c["name"].as_str().expect("column has name");
                let typ = c.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if typ.starts_with("ForeignKey") {
                    format!("{name}_id")
                } else {
                    name.to_string()
                }
            })
            .collect()
    }

    /// Copy a column const into an owned vec so assertions compare two
    /// runtime values.
    pub(crate) fn owned_columns(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }
}
