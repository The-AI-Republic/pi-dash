//! Hand-written `SeaORM` entity for the Rust-owned runner LLM model
//! catalog (NEWPIDASH-169).
//!
//! Unlike the generated entities in this module, this one covers a
//! `rust_*` runtime table: the OSS Rust backend owns it outright (created
//! at serve/worker boot by `pidash-api` — see
//! `crates/api/src/runner_models/schema.rs` — never through a Django
//! migration), so it has no Django counterpart and is deliberately absent
//! from the `parity_tests!` list in [`super::parity`]: the parity oracle
//! is a database migrated by `apps/api`, which never contains it. It is
//! also deliberately named with the `rust_` prefix: the Django migration
//! for the same feature (PDASHOSS01-364, PR 3177) owns a
//! `runner_model_catalog` table of its own, and the two names must never
//! collide.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "rust_runner_model_catalog")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub agent: String,
    pub model: String,
    pub label: String,
    pub reasoning_efforts: Vec<String>,
    pub sort_order: i32,
    pub is_enabled: bool,
    pub is_preselected: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
