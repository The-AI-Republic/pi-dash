#![forbid(unsafe_code)]

//! AWS credential resolution for the hand-rolled SigV4 clients.
//!
//! Moved to [`pidash_storage::aws_credentials`] in NEWPIDASH-204 so the
//! storage, services and jobs crates can share one implementation. This
//! module re-exports it for backwards compatibility (the KMS transport's
//! existing import path).

pub use pidash_storage::aws_credentials::*;
