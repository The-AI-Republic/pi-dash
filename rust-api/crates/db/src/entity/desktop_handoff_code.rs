//! `SeaORM` Entity for `desktop_handoff_code` (desktop_unified_signin §5).
//!
//! Schema owned by Django (`runner` migration `0043_desktop_handoff`);
//! created here for the Rust endpoints (Django stays schema owner, see
//! `db/src/migrations.rs`).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "desktop_handoff_code")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub code_hash: String,
    pub fingerprint: String,
    pub challenge: String,
    pub created_at: DateTimeWithTimeZone,
    pub expires_at: DateTimeWithTimeZone,
    pub consumed_at: Option<DateTimeWithTimeZone>,
    pub dev_machine_id: Uuid,
    /// The daemon credential the code was issued with (design §5.1): the
    /// code is refused once it is revoked or expired.
    pub machine_token_id: Option<Uuid>,
    pub user_id: Uuid,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
