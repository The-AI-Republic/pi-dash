#![forbid(unsafe_code)]

//! Background jobs: Postgres queue, worker loop, Celery-format publisher.
//!
//! The full queue and worker loop arrive under F-09. This scaffold defines
//! the wire format both sides speak during coexistence: tasks published here
//! are consumed by the existing Python Celery workers, so the JSON must match
//! Celery protocol v2 exactly.

pub mod celery;

pub use celery::CeleryTaskMessage;
