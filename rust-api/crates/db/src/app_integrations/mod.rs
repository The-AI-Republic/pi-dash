//! App integrations/webhooks table models (D-33, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/webhook.py:34-89` (`Webhook`,
//! `WebhookLog`, `ProjectWebhook`) and
//! `apps/api/pi_dash/db/models/integration/base.py:16-60` (`Integration`,
//! `WorkspaceIntegration`) for the db layer, adopting the Django-owned
//! schema column-for-column. Migrations are not ported; Django stays schema
//! owner until switchover.
//!
//! * [`models_webhook`] — the 5 tables (PIDASHCONV-380). `ProjectWebhook`
//!   has no owning sub-issue (it is absent from FX-MDL-01's 15 models and
//!   the cancelled siblings 390/403/418 do not cover it); it is ported here
//!   as the topological closure of `webhook.py`.
//!
//! The layout mirrors the D-05 models convention (PIDASHCONV-144,
//! `crate::integrations`): one `pub mod` per table with `TABLE`,
//! `ORDERING`, and `COLUMNS` consts in Django `_meta` field order, a row
//! struct, enum ports with stored-value round-trips, and FK / unique /
//! index consts. `#[cfg(test)]` asserts columns field-for-field against
//! `rust-api/fixtures/app_integrations/fx-mdl-01-model-columns.json`
//! (PIDASHCONV-337).
//!
//! # Column order and the fixture shape
//!
//! `COLUMNS` holds the audit prefix (`id`, `created_at`, `updated_at`,
//! `created_by_id`, `updated_by_id`, `deleted_at`; see [`AUDIT_COLUMNS`]),
//! then `project_id`, `workspace_id` for `ProjectBaseModel` tables (see
//! [`PROJECT_COLUMNS`]; `workspace` is auto-set from `project` on save,
//! `db/models/project.py:302-311`), then the declared fields in source
//! order with Django attnames (`workspace` -> `workspace_id`, ...). The
//! FX-MDL-01 fixture lists descriptive entries (including `/`-grouped
//! columns and `+ audit base` markers), so [`test_support`] expands them
//! to attnames before comparing. Order is cosmetic for query building;
//! membership is the contract.
//!
//! # Wiring note
//!
//! The crate root declares `pub mod app_integrations;` (one-line wiring in
//! the port PR, following the PIDASHCONV-284 precedent for `app_intake`).
//! Later D-33 layer issues add siblings here (`queries_webhook.rs` for
//! PIDASHCONV-428); the services-layer serializers (PIDASHCONV-365) live
//! under `crates/services/src/app_integrations/` and do not touch this
//! module.

pub mod models_webhook;

pub use models_webhook::{
    integration::Integration, project_webhook::ProjectWebhook, webhook::Webhook,
    webhook_log::WebhookLog, workspace_integration::WorkspaceIntegration, NetworkType, OnDelete,
};

/// Audit prefix shared by every model (`BaseModel` + `TimeAuditModel` +
/// `UserAuditModel` + `SoftDeleteModel`; matches the D-05
/// `integrations::AUDIT_COLUMNS` order).
pub const AUDIT_COLUMNS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "created_by_id",
    "updated_by_id",
    "deleted_at",
];

/// `ProjectBaseModel` prefix (`db/models/project.py:302-311`), inserted
/// between [`AUDIT_COLUMNS`] and the declared fields.
pub const PROJECT_COLUMNS: &[&str] = &["project_id", "workspace_id"];

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    /// Expand one FX-MDL-01 fixture `columns` entry into Django attnames.
    /// Entries are descriptive (`"workspace FK CASCADE related ..."`,
    /// `"project/issue/module/cycle/issue_comment/is_internal Bool False"`,
    /// `"+ audit base"`); `/`-separated heads expand to one attname each,
    /// and ` FK ` entries gain the `_id` suffix.
    pub(crate) fn fixture_attnames(entry: &str) -> Vec<String> {
        if entry == "+ audit base" {
            return super::AUDIT_COLUMNS
                .iter()
                .map(|c| (*c).to_string())
                .collect();
        }
        if let Some(group) = entry.strip_prefix("+ ") {
            return group
                .split('/')
                .map(|part| {
                    let part = part.trim();
                    if part == "created_by" || part == "updated_by" {
                        format!("{part}_id")
                    } else {
                        part.to_string()
                    }
                })
                .collect();
        }
        let head = entry.split([' ', '(']).next().unwrap_or_default();
        // `webhook UUIDField (NOT a FK ...)` names a plain UUID column, not
        // a foreign key, so it must not gain the `_id` suffix.
        let is_fk = entry.contains(" FK ") && !entry.contains("NOT a FK");
        head.split('/')
            .filter(|part| !part.is_empty())
            .map(|part| {
                if is_fk {
                    format!("{part}_id")
                } else {
                    part.to_string()
                }
            })
            .collect()
    }

    pub(crate) fn fixtures_path() -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/app_integrations/fx-mdl-01-model-columns.json")
    }

    pub(crate) fn fixture_model(name: &str) -> serde_json::Value {
        let body = std::fs::read_to_string(fixtures_path())
            .unwrap_or_else(|e| panic!("read fx-mdl-01-model-columns.json: {e}"));
        let v: serde_json::Value =
            serde_json::from_str(&body).expect("fx-mdl-01-model-columns.json is valid JSON");
        let m = v["models"][name].clone();
        assert!(m.is_object(), "fixture has model {name}");
        m
    }

    /// Declared-field attnames from the fixture, in fixture order.
    pub(crate) fn fixture_columns(name: &str) -> Vec<String> {
        fixture_model(name)["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("fixture {name} has columns array"))
            .iter()
            .flat_map(|c| {
                fixture_attnames(
                    c.as_str()
                        .unwrap_or_else(|| panic!("fixture {name} column is a string")),
                )
            })
            .collect()
    }

    pub(crate) fn fixture_table(name: &str) -> String {
        fixture_model(name)["table"].as_str().unwrap().to_string()
    }

    /// Copy a column const into an owned vec so assertions compare two
    /// runtime values.
    pub(crate) fn owned_columns(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| (*c).to_string()).collect()
    }
}
