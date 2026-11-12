//! License / instance-console domain surface (D-01, stage 3).
//!
//! Ports `apps/api/pi_dash/license/models/` for the db layer:
//!
//! * [`models`] — `instance.py` (`Instance`, `InstanceAdmin`,
//!   `InstanceConfiguration`, `ChangeLog`, `InstanceEdition`,
//!   `ROLE_CHOICES`; PIDASHCONV-116).
//!
//! Re-exports cover the `license/models/__init__.py:1-5` names (`Instance`,
//! `InstanceAdmin`, `InstanceConfiguration`, `InstanceEdition`) plus the
//! ported `ROLE_CHOICES` constants. `ChangeLog` is **not** re-exported,
//! mirroring Django where it is not importable via the package (only via
//! `pi_dash.license.models.instance`): it stays reachable through the full
//! `pidash_db::license::models::changelog` path. See the `models` module
//! docs for the ChangeLog-no-readers note.

pub mod models;
pub mod queries;

pub use models::{
    default_tags, instance::Instance, instance_admin::InstanceAdmin,
    instance_configuration::InstanceConfiguration, InstanceEdition, OnDelete, UnknownEdition,
    ADMIN_ROLE, DEFAULT_ROLE, ROLE_CHOICES,
};
