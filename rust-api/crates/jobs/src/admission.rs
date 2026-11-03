#![forbid(unsafe_code)]

//! Redis-backed [`AdmissionCache`](pidash_services::dispatch::admission::AdmissionCache)
//! for worker-owned dispatch (PIDASHCONV-1487).
//!
//! The implementation is unified in services
//! ([`RedisAdmissionCache`](pidash_services::dispatch::admission::RedisAdmissionCache),
//! OVL-06 PIDASHCONV-1544); this module re-exports it so worker-side
//! paths keep their import.

pub use pidash_services::dispatch::admission::RedisAdmissionCache;
