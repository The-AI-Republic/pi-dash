//! `SeaORM` Entity, hand-written for PDASHOSS01-364.
//!
//! Table `runner_model_catalog` is Django-owned (`runner.0040_runner_model_catalog`);
//! this entity is the Rust read/write surface over the same table. Columns match
//! `RunnerModelCatalog` in `apps/api/pi_dash/runner/models.py`: `id` UUID PK,
//! `agent` Char(32), `model`/`label` Char(128), `reasoning_efforts` JSON list,
//! `sort_order` PositiveInteger, `is_enabled`/`is_preselected` Bool,
//! `created_at`/`updated_at` timestamps, with partial uniques
//! `runner_model_unique_per_agent` and `runner_model_one_preselected_per_agent`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "runner_model_catalog")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub agent: String,
    pub model: String,
    pub label: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub reasoning_efforts: Json,
    pub sort_order: i32,
    pub is_enabled: bool,
    pub is_preselected: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
