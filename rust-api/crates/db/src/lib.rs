#![forbid(unsafe_code)]

//! Database connectivity for the Pi Dash Rust backend.
//!
//! Pool construction, sea-query builders, soft-delete views and the
//! transaction wrapper land here under F-04. This scaffold contributes the
//! connection configuration: parsing, validation, and redaction for logs.

pub mod config;

pub use config::DbConfig;
