#![forbid(unsafe_code)]

//! Redis-backed [`AdmissionCache`](pidash_services::run_creation::pidash_agent_availability::AdmissionCache)
//! for worker-owned dispatch (PIDASHCONV-1487).
//!
//! The implementation is unified in services
//! ([`RedisAdmissionCache`](pidash_services::run_creation::pidash_agent_availability::RedisAdmissionCache),
//! OVL-06 PIDASHCONV-1544); this module re-exports it so worker-side
//! paths keep their import.

pub use pidash_services::run_creation::pidash_agent_availability::RedisAdmissionCache;
