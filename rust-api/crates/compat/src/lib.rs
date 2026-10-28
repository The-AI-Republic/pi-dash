//! Public Python/DRF parity plumbing (OVL-04, PIDASHCONV-1542).
//!
//! Every `api` module used to keep private copies of these helpers; the
//! cloud overlay re-implemented them module by module, and the copies
//! drifted (the rust-v2026.10.26 release broke on copied sea-query
//! binders). Each family now has exactly one canonical implementation
//! here, re-exported through `pidash-sdk::compat` once OVL-02 lands.
//!
//! Rule: moved code, not rewritten code. Every function documents the
//! OSS module it was lifted from, and every OSS caller behaves
//! byte-identically after migration.

pub mod datetime;
pub mod db;
pub mod response;
pub mod validate;
