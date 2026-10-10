#![forbid(unsafe_code)]

//! Ops tail (D-37, stage 7): read-replica routing (PIDASHCONV-812,
//! fixture F37-11).
//!
//! [`routing`] ports the middleware half of replica routing —
//! `ReadReplicaRoutingMiddleware.__call__` / `process_view`, the
//! `use_read_replica` attribute lookup, and the
//! `ReadReplicaControlMixin` default — over the foundation kernels in
//! `pidash_db::pool` and `pidash_db::context` (read-only reuse).

pub mod routing;
